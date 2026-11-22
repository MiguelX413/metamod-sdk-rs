use metamod_sdk_rs::prelude::*;
use std::ffi::CString;

static MESSAGE: &ConsoleName = console_name!(c"RUST_HELLO_MESSAGE");

fn c_string(bytes: impl Into<Vec<u8>>) -> CString {
    CString::new(bytes).expect("demo text contains no NUL bytes")
}

#[derive(Default)]
struct EngineDemo {
    first_frame_seen: bool,
}

#[metamod_plugin(
    name = "Rust Engine Demo",
    version = "0.1.0",
    author = "metamod-sdk-rs contributors",
    log_tag = "RUSTENGINE"
)]
impl Plugin for EngineDemo {
    const CLIENT_CONNECT_PRE: bool = true;

    fn client_connect(
        &mut self,
        ctx: &mut Context<'_>,
        client: &ClientConnection,
    ) -> ClientAdmission {
        if client.name.as_c_str() == c"RustAdmissionReject" {
            ctx.server().print(c"Rust admission demo: rejected\n");
            ClientAdmission::Reject(c"Rust admission demo rejected this name.".to_owned())
        } else {
            ctx.server().print(c"Rust admission demo: deferred\n");
            ClientAdmission::Defer
        }
    }

    const KEYVALUE_PRE: bool = true;

    fn key_value(&mut self, ctx: &mut Context<'_>, event: &KeyValueEvent) {
        if event.key.as_c_str() == c"classname" {
            ctx.server().print(&c_string(format!(
                "Map classname: {}\n",
                event.value.to_string_lossy()
            )));
        }
    }

    const LAG_COMPENSATION_PRE: bool = true;
    const LAG_COMPENSATION_POST: bool = true;

    fn allow_lag_compensation(&mut self, _: &mut Context<'_>) -> HookResult<bool> {
        HookResult::Handled
    }

    fn allow_lag_compensation_post(
        &mut self,
        _: &mut Context<'_>,
        original: Option<bool>,
    ) -> PostHookResult<bool> {
        match original {
            Some(_) => PostHookResult::Handled,
            None => PostHookResult::Ignored,
        }
    }

    fn initialize(&mut self, ctx: &mut Context<'_>) -> bool {
        ctx.metamod()
            .print_console_trunc(c"Rust Engine Demo loaded");
        true
    }

    fn start_frame(&mut self, ctx: &mut Context<'_>) {
        if !self.first_frame_seen {
            self.first_frame_seen = true;
            let server = ctx.server();
            match server.cvar(MESSAGE) {
                Ok(Some(_)) => {}
                Ok(None) => {
                    if let Err(error) =
                        server.register_cvar(MESSAGE, cvar_value!(c"Hello from Rust!"))
                    {
                        server.print(&c_string(format!(
                            "Could not register rust_hello_message: {error}\n"
                        )));
                    }
                }
                Err(error) => server.print(&c_string(format!(
                    "Could not read rust_hello_message: {error}\n"
                ))),
            }
            if let Err(error) = server.register_command(console_name!(c"rust_hello")) {
                server.print(&c_string(format!(
                    "Could not register rust_hello: {error}\n"
                )));
            }
            if let Err(error) = server.register_command(console_name!(c"rust_trace")) {
                server.print(&c_string(format!(
                    "Could not register rust_trace: {error}\n"
                )));
            }
            if let Err(error) = server.register_command(console_name!(c"rust_entities")) {
                server.print(&c_string(format!(
                    "Could not register rust_entities: {error}\n"
                )));
            }
            server.print(c"Rust Engine Demo first frame\n");
        }
    }

    fn server_command(&mut self, ctx: &mut Context<'_>, command: &Command) {
        let server = ctx.server();
        if command.name().as_c_str() == c"rust_entities" {
            let entities = if let Some(argument) = command.arg(0) {
                let Ok(index) = argument.to_string_lossy().parse::<i32>() else {
                    server.print(c"rust_entities expects an integer edict index\n");
                    return;
                };
                ctx.entity_at(index)
                    .map(|entity| entity.into_iter().collect())
            } else {
                ctx.entities()
            };
            match entities {
                Ok(entities) => {
                    server.print(&c_string(format!("Visible entities: {}\n", entities.len())));
                    for entity in entities {
                        if let (Some(info), Some(spatial)) =
                            (ctx.entity_info(entity), ctx.entity_spatial(entity))
                        {
                            let classname = info
                                .keyvalues
                                .classname
                                .as_deref()
                                .map_or_else(|| "<unset>".into(), |name| name.to_string_lossy());
                            let targetname = info
                                .keyvalues
                                .targetname
                                .as_deref()
                                .map_or_else(|| "<unset>".into(), |name| name.to_string_lossy());
                            server.print(&c_string(format!(
                                "Entity {} {:?} class={} targetname={} origin: {:?}\n",
                                entity.index(),
                                info.kind,
                                classname,
                                targetname,
                                spatial.origin
                            )));
                        }
                    }
                }
                Err(error) => {
                    server.print(&c_string(format!("Entity discovery failed: {error}\n")))
                }
            }
            return;
        }
        if command.name().as_c_str() == c"rust_trace" {
            const SEGMENT: TraceSegment =
                match TraceSegment::new([0.0, 0.0, 64.0], [0.0, 0.0, -64.0]) {
                    Ok(segment) => segment,
                    Err(_) => panic!("demo trace endpoints must be finite"),
                };
            match server.trace_line(SEGMENT, TraceOptions::default()) {
                Ok(line) => {
                    let hit = line.hit.filter(|hit| ctx.is_entity_current(*hit));
                    server.print(&c_string(format!(
                        "Line fraction: {}, hit: {:?}\n",
                        line.fraction,
                        hit.map(EntityHandle::index)
                    )));
                    let options = TraceOptions {
                        monsters: TraceMonsters::Ignore,
                        skip: hit,
                    };
                    match server.trace_hull(SEGMENT, Hull::Human, options) {
                        Ok(hull) => {
                            server.print(&c_string(format!("Hull fraction: {}\n", hull.fraction)))
                        }
                        Err(error) => {
                            server.print(&c_string(format!("Hull trace failed: {error}\n")))
                        }
                    }
                }
                Err(error) => server.print(&c_string(format!("Line trace failed: {error}\n"))),
            }
            return;
        }
        if command.name().as_c_str() != c"rust_hello" {
            return;
        }
        if let Some(argument) = command.arg(0)
            && let Err(error) =
                CvarValue::new(argument).and_then(|value| server.set_cvar(MESSAGE, value))
        {
            server.print(&c_string(format!(
                "Could not update rust_hello_message: {error}\n"
            )));
            return;
        }
        match server.cvar(MESSAGE) {
            Ok(Some(message)) => {
                let mut line = message.into_bytes();
                if !line.ends_with(b"\n") {
                    line.push(b'\n');
                }
                server.print(&c_string(line));
            }
            Ok(None) => server.print(c"rust_hello_message is unavailable\n"),
            Err(error) => server.print(&c_string(format!(
                "Could not read rust_hello_message: {error}\n"
            ))),
        }
    }

    fn player_join(&mut self, ctx: &mut Context<'_>, player: PlayerId) {
        let info = ctx.player_info(player);
        if !ctx.is_player_current(player) {
            return;
        }
        let slot = player.slot();
        let server = ctx.server();
        if let Some(PlayerInfo {
            user_id: Some(user_id),
            ..
        }) = info
        {
            server.print(&c_string(format!(
                "Hello, player in slot {slot} (user ID {user_id})!\n"
            )));
        } else {
            server.print(&c_string(format!("Hello, player in slot {slot}!\n")));
        }
    }
}
