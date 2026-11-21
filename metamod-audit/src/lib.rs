use metamod_sdk_rs::prelude::*;
use std::{
    collections::{HashMap, HashSet},
    ffi::CString,
    fmt::Write as _,
    io::{self, Write as _},
    time::Instant,
};

const PREFIX: &str = "MM_AUDIT ";
const MAX_EVENTS: u32 = 4096;
const SAMPLE_COUNT: usize = 64;
const CVAR: &ConsoleName = console_name!(c"mm_audit_value");
const STATUS: &ConsoleName = console_name!(c"mm_audit_status");
const RUN: &ConsoleName = console_name!(c"mm_audit_run");
const REPORT: &ConsoleName = console_name!(c"mm_audit_report");

// Keep the protocol dependency-free. Native text is converted lossily and
// bounded before quoting, and is never used as a format string.
fn quoted(value: &str) -> String {
    let mut json = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => json.push_str("\\\""),
            '\\' => json.push_str("\\\\"),
            ch if ch.is_control() => {
                let _ = write!(json, "\\u{:04x}", u32::from(ch));
            }
            ch => json.push(ch),
        }
    }
    json.push('"');
    json
}

#[derive(Clone, Copy)]
struct Probe {
    id: &'static str,
    expected: &'static str,
}

const INIT: Probe = Probe {
    id: "init.v1",
    expected: "initialize callback reached",
};
const READY: Probe = Probe {
    id: "commands.register.v1",
    expected: "audit console commands submitted for registration",
};
const ACTIVATE: Probe = Probe {
    id: "map.activate.v1",
    expected: "post activation callback delivered",
};
const DEACTIVATE: Probe = Probe {
    id: "map.deactivate.v1",
    expected: "deactivation callback delivered",
};
const ORDER: Probe = Probe {
    id: "map.order.v1",
    expected: "deactivation precedes replacement activation",
};
const STALE: Probe = Probe {
    id: "map.stale.v1",
    expected: "previous map handle rejected",
};
const JOIN: Probe = Probe {
    id: "player.join.v1",
    expected: "joined session is current",
};
const DISCONNECT: Probe = Probe {
    id: "player.disconnect.v1",
    expected: "disconnected session is no longer current",
};
const REUSE: Probe = Probe {
    id: "player.reuse.v1",
    expected: "same slot gets a distinct ID after disconnect",
};
const IDENTITY: Probe = Probe {
    id: "player.identity.v1",
    expected: "owned player snapshot matches the session",
};
const REAL_REUSE: Probe = Probe {
    id: "player.real_reuse.v1",
    expected: "non-fake clients disconnect and reuse a slot with distinct session identities",
};
const KEYS: Probe = Probe {
    id: "entity.keyvalue_hook.v1",
    expected: "classname callback provides owned nonempty bytes",
};
const DISCOVER: Probe = Probe {
    id: "entity.discovery.v1",
    expected: "active world appears in bounded discovery",
};
const SNAPSHOT: Probe = Probe {
    id: "entity.snapshot.v1",
    expected: "world spatial and keyvalue snapshots refer to the same handle",
};
const CVARS: Probe = Probe {
    id: "cvar.roundtrip.v1",
    expected: "private CVar registers and round-trips a write",
};
const STRINGS: Probe = Probe {
    id: "string.copy.v1",
    expected: "owned CVar read survives a later write",
};
const LIMIT: Probe = Probe {
    id: "string.limit.v1",
    expected: "1023-byte CVar value accepted and 1024-byte value rejected by SDK",
};
const ARGS: Probe = Probe {
    id: "command.arguments.v1",
    expected: "quoted argument delivered as one exact token",
};
const LINE: Probe = Probe {
    id: "trace.line.v1",
    expected: "line trace yields a finite fraction in range",
};
const HULL: Probe = Probe {
    id: "trace.hull.v1",
    expected: "hull trace yields a finite fraction in range",
};
const LAG_PRE: Probe = Probe {
    id: "lag.pre.v1",
    expected: "lag pre callback delivered",
};
const LAG_POST: Probe = Probe {
    id: "lag.post.v1",
    expected: "lag post callback reports whether an original result exists",
};
const HOOK: Probe = Probe {
    id: "hook.result.v1",
    expected: "host interprets hook status and return slot correctly",
};
const REENTER: Probe = Probe {
    id: "dispatch.reentrant.v1",
    expected: "nested notifications delivered after active callback",
};
const ERRORS: Probe = Probe {
    id: "errors.v1",
    expected: "negative edict index and unknown private CVar handled safely",
};
const SHUTDOWN: Probe = Probe {
    id: "shutdown.v1",
    expected: "shutdown callback delivered",
};
const DROP: Probe = Probe {
    id: "shutdown.drop.v1",
    expected: "instance destructor entered after shutdown callback",
};
const REPEAT: Probe = Probe {
    id: "resources.repeat.v1",
    expected: "64 repeated private CVar reads complete",
};

const PASSIVE: &[(Probe, &str)] = &[
    (ACTIVATE, "requires a forwarded activation"),
    (DEACTIVATE, "requires a forwarded deactivation"),
    (ORDER, "requires two observed activations"),
    (STALE, "requires a captured world handle and a map change"),
    (JOIN, "requires a client connection"),
    (DISCONNECT, "requires a client disconnection"),
    (REUSE, "requires a disconnect and same-slot join on one map"),
    (IDENTITY, "requires a client identity snapshot"),
    (
        REAL_REUSE,
        "requires non-fake clients to reuse a slot on one map",
    ),
    (KEYS, "requires a forwarded classname keyvalue"),
    (LAG_PRE, "requires a forwarded lag callback"),
    (LAG_POST, "requires a forwarded lag callback"),
    (HOOK, "requires native harness or host-side instrumentation"),
    (
        REENTER,
        "requires native harness or host-side instrumentation",
    ),
    (SHUTDOWN, "requires forwarded GameShutdown after report"),
    (DROP, "requires instance destruction after shutdown"),
];

#[derive(Default)]
struct Audit {
    sequence: u32,
    seen: HashMap<&'static str, &'static str>,
    ready: bool,
    cvar_registered: bool,
    activated: bool,
    deactivated: bool,
    world: Option<EntityHandle>,
    disconnected: HashMap<PlayerSlot, (PlayerId, bool)>,
    real_players: HashSet<PlayerId>,
    shutdown: bool,
}

impl Audit {
    fn emit(&mut self, context: Option<&Context<'_>>, kind: &str, fields: &str) {
        if self.sequence >= MAX_EVENTS {
            return;
        }
        self.sequence += 1;
        let (kind, fields) = if self.sequence == MAX_EVENTS {
            ("limit", "")
        } else {
            (kind, fields)
        };
        let line = format!(
            "{PREFIX}{{\"schema_version\":1,\"seq\":{},\"kind\":{}{fields}}}\n",
            self.sequence,
            quoted(kind),
        );
        if let Some(context) = context {
            context
                .server()
                .print(&CString::new(line).expect("JSON has no raw NUL"));
        } else {
            // Engine callbacks have been revoked. A failed diagnostic write
            // must not interfere with cleanup.
            let _ = io::stderr().write_all(line.as_bytes());
        }
    }

    fn result(
        &mut self,
        context: Option<&Context<'_>>,
        probe: Probe,
        status: &'static str,
        observed: impl AsRef<str>,
    ) {
        let observed: String = observed.as_ref().chars().take(160).collect();
        let fields = format!(
            ",\"test\":{},\"result\":{},\"expected\":{},\"observed\":{}",
            quoted(probe.id),
            quoted(status),
            quoted(probe.expected),
            quoted(&observed)
        );
        self.seen.insert(probe.id, status);
        self.emit(context, "result", &fields);
    }

    fn check(&mut self, ctx: &Context<'_>, probe: Probe, ok: bool, evidence: impl AsRef<str>) {
        self.result(Some(ctx), probe, if ok { "PASS" } else { "FAIL" }, evidence);
    }

    fn report(&mut self, ctx: &Context<'_>) {
        for &(probe, reason) in PASSIVE {
            if !self.seen.contains_key(probe.id) {
                self.result(Some(ctx), probe, "SKIP", reason);
            }
        }
        self.emit(Some(ctx), "report", ",\"complete\":true");
    }

    fn console(&mut self, ctx: &Context<'_>) {
        let server = ctx.server();
        let first = server.cvar(CVAR);
        if self.cvar_registered
            && let Ok(Some(previous)) = first
        {
            let prior_bytes = previous.as_bytes().to_vec();
            let marker = if previous.as_c_str() == c"audit-a" {
                c"audit-b"
            } else {
                c"audit-a"
            };
            let write = server.set_cvar(CVAR, CvarValue::new(marker).expect("short marker"));
            let after = server.cvar(CVAR);
            let matched =
                write.is_ok() && after.as_ref().ok().and_then(|v| v.as_deref()) == Some(marker);
            self.check(
                ctx,
                CVARS,
                matched,
                format!("write={write:?}, marker matches={matched}"),
            );
            if matched {
                self.check(
                    ctx,
                    STRINGS,
                    previous.as_bytes() == prior_bytes,
                    format!(
                        "prior copy retained {} bytes after native write",
                        prior_bytes.len()
                    ),
                );
            } else {
                self.result(
                    Some(ctx),
                    STRINGS,
                    "INCONCLUSIVE",
                    "native write did not establish a different value",
                );
            }
            if let Ok(previous) = CvarValue::new(previous.as_c_str()) {
                let _ = server.set_cvar(CVAR, previous);
            }
        } else {
            self.result(
                Some(ctx),
                CVARS,
                if self.cvar_registered { "FAIL" } else { "SKIP" },
                "private CVar could not be read",
            );
            self.result(
                Some(ctx),
                STRINGS,
                "SKIP",
                "private CVar round trip unavailable",
            );
        }
        let at_limit = CString::new(vec![b'x'; 1023]).expect("no NUL");
        let above_limit = CString::new(vec![b'x'; 1024]).expect("no NUL");
        self.check(
            ctx,
            LIMIT,
            CvarValue::new(at_limit.as_c_str()).is_ok()
                && CvarValue::new(above_limit.as_c_str()).is_err(),
            "validated local inputs at 1023 and 1024 bytes",
        );
        let missing = server.cvar(console_name!(c"mm_audit_unregistered_probe"));
        let negative = ctx.entity_at(-1);
        let invalid_index_safe = matches!(negative, Ok(None) | Err(EntityError::Unavailable));
        match missing {
            Ok(None) => self.check(
                ctx,
                ERRORS,
                invalid_index_safe,
                format!("negative index={negative:?}; unknown CVar absent"),
            ),
            other => self.result(
                Some(ctx),
                ERRORS,
                "INCONCLUSIVE",
                format!(
                    "unknown CVar lookup did not return absence; available={}",
                    other.is_ok()
                ),
            ),
        }
    }

    fn entities(&mut self, ctx: &Context<'_>) {
        let world = ctx.entity_at(0);
        let entities = ctx.entities();
        match (world, entities) {
            (Ok(Some(world)), Ok(entities)) => {
                self.check(
                    ctx,
                    DISCOVER,
                    entities.contains(&world),
                    format!("{} live handles", entities.len()),
                );
                let info = ctx.entity_info(world);
                let spatial = ctx.entity_spatial(world);
                let matched = info
                    .as_ref()
                    .is_some_and(|i| i.entity == world && i.kind == EntityKind::World)
                    && spatial.as_ref().is_some_and(|s| s.entity == world);
                self.result(
                    Some(ctx),
                    SNAPSHOT,
                    if matched { "PASS" } else { "INCONCLUSIVE" },
                    format!(
                        "keyvalue snapshot={}, classname={}, spatial snapshot={}",
                        info.is_some(),
                        info.is_some_and(|i| i.keyvalues.classname.is_some()),
                        spatial.is_some()
                    ),
                );
            }
            (Ok(None), Ok(entities)) => {
                self.check(
                    ctx,
                    DISCOVER,
                    false,
                    format!("world missing from {} live handles", entities.len()),
                );
                self.result(Some(ctx), SNAPSHOT, "SKIP", "world handle unavailable");
            }
            (world, entities) => {
                self.result(
                    Some(ctx),
                    DISCOVER,
                    "INCONCLUSIVE",
                    format!("world={world:?}, discovery error={:?}", entities.err()),
                );
                self.result(
                    Some(ctx),
                    SNAPSHOT,
                    "SKIP",
                    "stable world handle unavailable",
                );
            }
        }
    }

    fn traces(&mut self, ctx: &Context<'_>) {
        let segment =
            TraceSegment::new([0.0, 0.0, 64.0], [0.0, 0.0, -64.0]).expect("finite endpoints");
        let server = ctx.server();
        for (probe, trace) in [
            (LINE, server.trace_line(segment, TraceOptions::default())),
            (
                HULL,
                server.trace_hull(segment, Hull::Human, TraceOptions::default()),
            ),
        ] {
            match trace {
                Ok(hit) => self.check(
                    ctx,
                    probe,
                    (0.0..=1.0).contains(&hit.fraction),
                    format!("fraction={}, hit={}", hit.fraction, hit.hit.is_some()),
                ),
                Err(err) => self.result(Some(ctx), probe, "INCONCLUSIVE", err.to_string()),
            }
        }
    }

    fn repeat(&mut self, ctx: &Context<'_>) {
        if !self.cvar_registered {
            self.result(Some(ctx), REPEAT, "SKIP", "private CVar not registered");
            return;
        }
        let now = Instant::now();
        let count = (0..SAMPLE_COUNT)
            .take_while(|_| matches!(ctx.server().cvar(CVAR), Ok(Some(_))))
            .count();
        self.check(
            ctx,
            REPEAT,
            count == SAMPLE_COUNT,
            format!(
                "{count}/{SAMPLE_COUNT} owned reads in {} us",
                now.elapsed().as_micros()
            ),
        );
    }
}

#[metamod_plugin(
    name = "Rust Runtime Audit",
    version = "0.1.0",
    author = "metamod-sdk-rs contributors",
    log_tag = "RUSTAUDIT"
)]
impl Plugin for Audit {
    const KEYVALUE_PRE: bool = true;
    const LAG_COMPENSATION_PRE: bool = true;
    const LAG_COMPENSATION_POST: bool = true;

    fn initialize(&mut self, ctx: &mut Context<'_>) -> bool {
        self.check(ctx, INIT, true, "initialize");
        true
    }

    fn start_frame(&mut self, ctx: &mut Context<'_>) {
        if self.ready {
            return;
        }
        self.ready = true;
        let server = ctx.server();
        self.cvar_registered = server
            .register_cvar(CVAR, cvar_value!(c"audit-default"))
            .is_ok();
        let registrations = [STATUS, RUN, REPORT].map(|name| server.register_command(name));
        let submitted = registrations.iter().all(Result::is_ok);
        self.check(ctx, READY, submitted, format!("{registrations:?}"));
        self.emit(
            Some(ctx),
            "ready",
            if submitted {
                ",\"submitted\":true"
            } else {
                ",\"submitted\":false"
            },
        );
    }

    fn server_command(&mut self, ctx: &mut Context<'_>, command: &Command) {
        match command.name().as_c_str() {
            name if name == STATUS.as_c_str() => {
                self.emit(Some(ctx), "status", ",\"callback\":\"server_command\"")
            }
            name if name == REPORT.as_c_str() => self.report(ctx),
            name if name == RUN.as_c_str() => {
                let suite = command.arg(0).unwrap_or(c"all");
                if let Some(marker) = command.arg(1) {
                    self.check(
                        ctx,
                        ARGS,
                        marker == c"audit two words" && command.args().len() == 2,
                        format!(
                            "{} operands, marker length {}",
                            command.args().len(),
                            marker.to_bytes().len()
                        ),
                    );
                }
                match suite.to_bytes() {
                    b"all" => {
                        self.console(ctx);
                        self.entities(ctx);
                        self.traces(ctx);
                        self.repeat(ctx);
                    }
                    b"console" => self.console(ctx),
                    b"entities" => self.entities(ctx),
                    b"trace" => self.traces(ctx),
                    b"resources" => self.repeat(ctx),
                    _ => {
                        self.emit(
                            Some(ctx),
                            "usage",
                            ",\"suites\":\"all console entities trace resources\"",
                        );
                        return;
                    }
                }
                self.emit(
                    Some(ctx),
                    "suite_complete",
                    &format!(",\"suite\":{}", quoted(&suite.to_string_lossy())),
                );
            }
            _ => {}
        }
    }

    fn server_activate(&mut self, ctx: &mut Context<'_>) {
        self.check(ctx, ACTIVATE, true, "server_activate");
        if self.activated {
            self.result(
                Some(ctx),
                ORDER,
                if self.deactivated {
                    "PASS"
                } else {
                    "INCONCLUSIVE"
                },
                if self.deactivated {
                    "deactivation followed by activation"
                } else {
                    "no deactivation observed between two activations"
                },
            );
        }
        if let Some(old) = self.world.take() {
            self.check(
                ctx,
                STALE,
                !ctx.is_entity_current(old),
                "checked previous world on activation",
            );
        }
        self.world = ctx.entity_at(0).ok().flatten();
        self.activated = true;
        self.deactivated = false;
        self.disconnected.clear();
        self.real_players.clear();
    }

    fn server_deactivate(&mut self, ctx: &mut Context<'_>) {
        self.deactivated = true;
        self.disconnected.clear();
        self.real_players.clear();
        self.check(ctx, DEACTIVATE, true, "server_deactivate");
        if let Some(old) = self.world {
            self.check(
                ctx,
                STALE,
                !ctx.is_entity_current(old),
                "checked previous world during deactivation",
            );
        }
    }

    fn player_join(&mut self, ctx: &mut Context<'_>, player: PlayerId) {
        let slot = player.slot();
        let real = ctx
            .entity_at(slot.get())
            .ok()
            .flatten()
            .and_then(|entity| ctx.entity_info(entity))
            .is_some_and(|info| info.kind == EntityKind::Client)
            && ctx.is_player_current(player);
        self.result(
            Some(ctx),
            JOIN,
            if ctx.is_player_current(player) {
                "PASS"
            } else {
                "INCONCLUSIVE"
            },
            format!("player_join slot {slot}"),
        );
        let real_reuse = if let Some((previous, previous_real)) = self.disconnected.remove(&slot) {
            self.check(
                ctx,
                REUSE,
                previous != player && !ctx.is_player_current(previous),
                format!("slot {slot} reused"),
            );
            real && previous_real && previous != player && !ctx.is_player_current(previous)
        } else {
            false
        };
        match ctx.player_info(player) {
            Some(info) => {
                if real && info.id == player {
                    self.real_players.retain(|id| id.slot() != slot);
                    self.real_players.insert(player);
                }
                self.check(
                    ctx,
                    IDENTITY,
                    info.id == player,
                    format!(
                        "slot {slot}, user ID present={}, auth ID present={}",
                        info.user_id.is_some(),
                        info.auth_id.is_some()
                    ),
                );
                if real_reuse && info.id == player {
                    self.check(
                        ctx,
                        REAL_REUSE,
                        true,
                        format!("non-fake slot {slot} reused"),
                    );
                }
            }
            None => self.result(
                Some(ctx),
                IDENTITY,
                "INCONCLUSIVE",
                format!("slot {slot} snapshot unavailable"),
            ),
        }
    }

    fn player_disconnect(&mut self, ctx: &mut Context<'_>, player: PlayerId) {
        let real = self.real_players.remove(&player);
        if self.disconnected.len() < 64 {
            self.disconnected.insert(
                player.slot(),
                (player, real && !ctx.is_player_current(player)),
            );
        }
        self.check(
            ctx,
            DISCONNECT,
            !ctx.is_player_current(player),
            format!("player_disconnect slot {}", player.slot()),
        );
    }

    fn key_value(&mut self, ctx: &mut Context<'_>, event: &KeyValueEvent) {
        if event.key.as_c_str() == c"classname"
            && self
                .seen
                .get(KEYS.id)
                .is_none_or(|status| *status == "SKIP")
        {
            self.check(
                ctx,
                KEYS,
                !event.value.as_bytes().is_empty(),
                format!("classname length {}", event.value.as_bytes().len()),
            );
        }
    }

    fn allow_lag_compensation(&mut self, ctx: &mut Context<'_>) -> HookResult<bool> {
        if self
            .seen
            .get(LAG_PRE.id)
            .is_none_or(|status| *status == "SKIP")
        {
            self.check(
                ctx,
                LAG_PRE,
                true,
                "allow_lag_compensation: returning Ignored",
            );
        }
        HookResult::Ignored
    }

    fn allow_lag_compensation_post(
        &mut self,
        ctx: &mut Context<'_>,
        original: Option<bool>,
    ) -> PostHookResult<bool> {
        if self
            .seen
            .get(LAG_POST.id)
            .is_none_or(|status| *status == "SKIP")
        {
            self.check(
                ctx,
                LAG_POST,
                true,
                format!("allow_lag_compensation_post: original={original:?}"),
            );
        }
        PostHookResult::Ignored
    }

    fn shutdown(&mut self) {
        self.shutdown = true;
        self.result(None, SHUTDOWN, "PASS", "GameShutdown reached Rust cleanup");
    }
}

impl Drop for Audit {
    fn drop(&mut self) {
        if self.shutdown {
            self.result(None, DROP, "PASS", "Drop entered after shutdown callback");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_escapes_console_control_characters() {
        assert_eq!(quoted("a\"\\\0\nπ"), r#""a\"\\\u0000\u000aπ""#);
    }
}
