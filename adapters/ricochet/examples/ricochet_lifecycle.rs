//! Test-only plugin for the isolated, verified stock Ricochet snapshots.

use metamod_ricochet::Ricochet;
use metamod_sdk_rs::{FakeClient, FakeClientState, prelude::*};

#[derive(Default)]
struct Lifecycle {
    client: Option<FakeClient>,
    first: Option<PlayerId>,
    finished: bool,
}

impl Plugin for Lifecycle {
    const METADATA: Metadata = Metadata {
        name: c"SDK Ricochet lifecycle test",
        version: c"1",
        date: c"",
        author: c"metamod-sdk-rs",
        url: c"",
        log_tag: c"RICOLIFE",
    };

    fn start_frame(&mut self, ctx: &mut Context<'_>) {
        if self.finished {
            return;
        }
        if let Some(client) = &self.client {
            match client.state() {
                FakeClientState::RemovalQueued => return,
                FakeClientState::Removed if self.first == Some(client.player()) => {
                    self.client = None;
                }
                FakeClientState::Removed => {
                    self.finished = true;
                    ctx.server()
                        .print(c"FAKE_STOCK PASS SDK confirmed both removals\n");
                    return;
                }
                _ => {
                    self.finished = true;
                    ctx.server()
                        .print(c"FAKE_STOCK FAIL unexpected lifecycle state\n");
                    return;
                }
            }
        }
        // SAFETY: This probe is launched exclusively by the stock test runner
        // with the documented engine, classic Metamod and stock Ricochet.
        let adapter = unsafe { Ricochet::new() };
        let server = ctx.server();
        let Ok(client) = adapter.create_fake_client(&server, c"SDKLifecycleProbe") else {
            self.finished = true;
            server.print(c"FAKE_STOCK FAIL SDK setup\n");
            return;
        };
        let player = client.player();
        if !ctx.is_player_current(player)
            || self
                .first
                .is_some_and(|first| first.slot() != player.slot() || first == player)
            || server.request_fake_client_removal(&client).is_err()
            || client.state() != FakeClientState::RemovalQueued
        {
            self.finished = true;
            server.print(c"FAKE_STOCK FAIL SDK identity or teardown request\n");
            return;
        }
        self.first.get_or_insert(player);
        self.client = Some(client);
    }
}

export_plugin!(Lifecycle);
