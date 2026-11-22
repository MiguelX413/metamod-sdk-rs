//! Test-only plugin for the isolated stock Ricochet map-change probe.

use metamod_ricochet::Ricochet;
use metamod_sdk_rs::{FakeClient, FakeClientError, FakeClientState, prelude::*};

const CHANGE: &ConsoleName = console_name!(c"sdk_fake_map_probe");

#[derive(Default)]
struct MapProbe {
    registered: bool,
    finished: bool,
    activations: usize,
    old_disconnect_seen: bool,
    old: Option<FakeClient>,
    replacement: Option<FakeClient>,
}

impl MapProbe {
    fn fail(&mut self, ctx: &Context<'_>, message: &'static std::ffi::CStr) {
        self.finished = true;
        ctx.server().print(message);
    }

    fn create(ctx: &Context<'_>, name: &std::ffi::CStr) -> Result<FakeClient, FakeClientError> {
        // SAFETY: The test runner supplies one of the verified stock Ricochet
        // deployments. This example deliberately exercises its map lifecycle.
        unsafe { Ricochet::new() }.create_fake_client(&ctx.server(), name)
    }
}

impl Plugin for MapProbe {
    const METADATA: Metadata = Metadata {
        name: c"SDK Ricochet map-change test",
        version: c"1",
        date: c"",
        author: c"metamod-sdk-rs",
        url: c"",
        log_tag: c"RICOMAP",
    };

    fn server_activate(&mut self, _: &mut Context<'_>) {
        self.activations += 1;
    }

    fn server_command(&mut self, ctx: &mut Context<'_>, command: &Command) {
        if self.finished || self.old.is_some() || command.name() != CHANGE {
            return;
        }
        let Ok(client) = Self::create(ctx, c"SDKMapOld") else {
            return self.fail(ctx, c"FAKE_MAP FAIL initial setup\n");
        };
        if self.activations != 1
            || ctx.server().request_fake_client_removal(&client).is_err()
            || client.state() != FakeClientState::RemovalQueued
        {
            return self.fail(ctx, c"FAKE_MAP FAIL initial removal was not queued\n");
        }
        self.old = Some(client);
        ctx.server()
            .print(c"FAKE_MAP EVENT removal queued on first map\n");
    }

    fn server_deactivate(&mut self, ctx: &mut Context<'_>) {
        let Some(old) = &self.old else { return };
        if self.finished {
            return;
        }
        if !self.old_disconnect_seen
            || old.state() != FakeClientState::Invalidated
            || ctx.is_player_current(old.player())
        {
            return self.fail(
                ctx,
                c"FAKE_MAP FAIL pending removal survived deactivation\n",
            );
        }
        ctx.server()
            .print(c"FAKE_MAP EVENT old handle invalidated at deactivation\n");
    }

    fn player_disconnect(&mut self, ctx: &mut Context<'_>, player: PlayerId) {
        if self.old.as_ref().is_some_and(|old| {
            old.player() == player && old.state() == FakeClientState::RemovalQueued
        }) {
            self.old_disconnect_seen = true;
            ctx.server()
                .print(c"FAKE_MAP EVENT old disconnect while removal pending\n");
        }
    }

    fn start_frame(&mut self, ctx: &mut Context<'_>) {
        if self.finished {
            return;
        }
        if !self.registered {
            if ctx.server().register_command(CHANGE).is_err() {
                return self.fail(ctx, c"FAKE_MAP FAIL command registration\n");
            }
            self.registered = true;
            ctx.server().print(c"FAKE_MAP READY\n");
        }
        if let Some(client) = &self.replacement {
            match client.state() {
                FakeClientState::RemovalQueued => return,
                FakeClientState::Removed => {
                    self.finished = true;
                    ctx.server()
                        .print(c"FAKE_MAP PASS replacement removal confirmed\n");
                }
                _ => self.fail(ctx, c"FAKE_MAP FAIL unexpected replacement state\n"),
            }
            return;
        }
        let Some(old) = &self.old else { return };
        if self.activations != 2 {
            if old.state() == FakeClientState::Removed {
                self.fail(ctx, c"FAKE_MAP FAIL kick completed before changelevel\n");
            }
            return;
        }
        if old.state() != FakeClientState::Invalidated
            || ctx.server().request_fake_client_removal(old) != Err(FakeClientError::NotActive)
        {
            return self.fail(ctx, c"FAKE_MAP FAIL stale handle could request removal\n");
        }
        let old_player = old.player();
        let Ok(replacement) = Self::create(ctx, c"SDKMapReplacement") else {
            return self.fail(ctx, c"FAKE_MAP FAIL replacement setup\n");
        };
        let new_player = replacement.player();
        if new_player.slot() != old_player.slot()
            || new_player == old_player
            || ctx.is_player_current(old_player)
            || !ctx.is_player_current(new_player)
        {
            return self.fail(
                ctx,
                c"FAKE_MAP FAIL expected same slot and distinct session\n",
            );
        }
        ctx.server()
            .print(c"FAKE_MAP EVENT reused slot with distinct PlayerId\n");
        if ctx
            .server()
            .request_fake_client_removal(&replacement)
            .is_err()
            || replacement.state() != FakeClientState::RemovalQueued
        {
            return self.fail(ctx, c"FAKE_MAP FAIL replacement removal was not queued\n");
        }
        self.replacement = Some(replacement);
    }
}

export_plugin!(MapProbe);
