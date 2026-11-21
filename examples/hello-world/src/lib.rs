use metamod_sdk_rs::prelude::*;
use std::ffi::CString;

#[derive(Default)]
struct HelloWorld {
    first_frame_seen: bool,
}

#[metamod_plugin(cargo)]
impl Plugin for HelloWorld {
    fn initialize(&mut self, ctx: &mut Context<'_>) -> bool {
        let server = ctx.server();
        server.print(c"Hello from Rust Metamod");
        server.print(c"\n");
        true
    }

    fn start_frame(&mut self, ctx: &mut Context<'_>) {
        if !self.first_frame_seen {
            self.first_frame_seen = true;
            ctx.server().print(c"Rust StartFrame hook ran\n");
        }
    }

    fn player_join(&mut self, ctx: &mut Context<'_>, player: PlayerId) {
        if ctx.is_player_current(player)
            && let Ok(message) = CString::new(format!("Hello, player in slot {}!\n", player.slot()))
        {
            ctx.server().print(&message);
        }
    }
}

#[cfg(test)]
mod tests;
