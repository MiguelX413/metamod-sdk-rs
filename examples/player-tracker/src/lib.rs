use metamod_sdk_rs::prelude::*;
use std::{collections::HashSet, ffi::CString};

#[derive(Default)]
struct PlayerTracker {
    registration_attempted: bool,
    players: HashSet<PlayerId>,
    watched: Option<PlayerId>,
}

#[metamod_plugin(cargo)]
impl Plugin for PlayerTracker {
    fn start_frame(&mut self, ctx: &mut Context<'_>) {
        if self.registration_attempted {
            return;
        }
        self.registration_attempted = true;
        for name in [
            console_name!(c"rust_sessions"),
            console_name!(c"rust_watch"),
            console_name!(c"rust_watched"),
        ] {
            if let Err(error) = ctx.server().register_command(name) {
                eprintln!("player-tracker: could not register {name:?}: {error}");
            }
        }
    }

    fn player_join(&mut self, ctx: &mut Context<'_>, player: PlayerId) {
        // A deferred join may describe a session that has already ended.
        if ctx.is_player_current(player) {
            self.players.insert(player);
            print(ctx, format!("Player joined slot {}\n", player.slot()));
        }
    }

    fn player_disconnect(&mut self, ctx: &mut Context<'_>, player: PlayerId) {
        self.players.remove(&player);
        print(
            ctx,
            format!("Player disconnected from slot {}\n", player.slot()),
        );
        // Retain the selection so rust_watched can report that it expired.
    }

    fn server_deactivate(&mut self, _: &mut Context<'_>) {
        self.players.clear();
        self.watched = None;
    }

    fn server_command(&mut self, ctx: &mut Context<'_>, command: &Command) {
        match command.name().as_c_str().to_bytes() {
            b"rust_sessions" => {
                self.players.retain(|id| ctx.is_player_current(*id));
                let mut slots: Vec<_> = self.players.iter().map(|id| id.get()).collect();
                slots.sort_unstable();
                print(ctx, format!("Observed player slots: {slots:?}\n"));
            }
            b"rust_watch" => {
                let Some(slot) = command
                    .arg(0)
                    .and_then(|arg| arg.to_str().ok())
                    .and_then(|text| text.parse::<PlayerSlot>().ok())
                else {
                    ctx.server()
                        .print(c"Usage: rust_watch <positive player slot>\n");
                    return;
                };
                match ctx.player_at(slot) {
                    Some(player) => {
                        self.watched = Some(player);
                        print(
                            ctx,
                            format!("Watching the current session in slot {slot}\n"),
                        );
                    }
                    None => print(ctx, format!("No observed player session in slot {slot}\n")),
                }
            }
            b"rust_watched" => show_watched(ctx, self.watched),
            _ => {}
        }
    }
}

fn show_watched(ctx: &Context<'_>, watched: Option<PlayerId>) {
    let Some(player) = watched else {
        ctx.server()
            .print(c"No selection. Use rust_watch <slot> first.\n");
        return;
    };
    if !ctx.is_player_current(player) {
        ctx.server()
            .print(c"The watched session ended. Select a session again with rust_watch.\n");
        return;
    }
    let info = ctx.player_info(player);
    // The native information lookup can reenter and replace the slot occupant.
    if !ctx.is_player_current(player) {
        ctx.server()
            .print(c"The watched session ended during lookup.\n");
        return;
    }
    match info.and_then(|info| info.user_id) {
        Some(user_id) => print(
            ctx,
            format!(
                "Watched session: slot {}, native user ID {user_id}\n",
                player.slot()
            ),
        ),
        None => ctx
            .server()
            .print(c"The watched session is tracked, but native information is unavailable.\n"),
    }
}

fn print(ctx: &Context<'_>, text: String) {
    // Only numbers and fixed text are formatted; no native strings enter this conversion.
    let text = CString::new(text).expect("tracker output contains no NUL bytes");
    ctx.server().print(&text);
}
