//! Test-only relationship assertions for verify/entity-relationships/fixture.cpp.
use metamod_sdk_rs::prelude::*;

#[derive(Default)]
struct Relationships {
    registered: bool,
    announced: bool,
    phase: u8,
    source: Option<EntityHandle>,
    target: Option<EntityHandle>,
    deactivated: bool,
}

#[metamod_plugin(
    name = "SDK Relationship Probe",
    version = "1",
    author = "metamod-sdk-rs contributors",
    log_tag = "RELPROBE"
)]
impl Plugin for Relationships {
    fn start_frame(&mut self, ctx: &mut Context<'_>) {
        if !self.registered {
            self.registered = ctx
                .server()
                .register_command(console_name!(c"sdk_relationship_check"))
                .is_ok();
        }
        if self.registered && !self.announced {
            self.announced = true;
            ctx.server().print(c"RELATIONSHIP READY\n");
        }
    }

    fn server_command(&mut self, ctx: &mut Context<'_>, command: &Command) {
        let passed = self.check(ctx, command);
        match (passed, self.phase) {
            (true, 1) => ctx.server().print(c"RELATIONSHIP LIVE PASS\n"),
            (true, 2) => ctx.server().print(c"RELATIONSHIP RETIRED PASS\n"),
            (true, 3) => ctx.server().print(c"RELATIONSHIP REUSED PASS\n"),
            _ => ctx
                .server()
                .print(c"RELATIONSHIP FAIL snapshot assertion\n"),
        }
    }

    fn server_deactivate(&mut self, ctx: &mut Context<'_>) {
        self.deactivated = self.phase == 3
            && self.source.is_some_and(|source| {
                !ctx.is_entity_current(source) && ctx.entity_relationships(source).is_none()
            });
    }

    fn server_activate(&mut self, ctx: &mut Context<'_>) {
        if let Some(source) = self.source {
            let passed = self.deactivated
                && !ctx.is_entity_current(source)
                && ctx.entity_relationships(source).is_none()
                && self
                    .target
                    .is_some_and(|target| !ctx.is_entity_current(target));
            ctx.server().print(if passed {
                c"RELATIONSHIP MAP PASS\n"
            } else {
                c"RELATIONSHIP FAIL stale map identity\n"
            });
        }
        self.phase = 0;
        self.announced = false;
    }
}

impl Relationships {
    fn check(&mut self, ctx: &Context<'_>, command: &Command) -> bool {
        let index = |argument| command.arg(argument)?.to_str().ok()?.parse::<i32>().ok();
        let (Some(source_index), Some(target_index), Some(phase)) =
            (index(0), index(1), command.arg(2))
        else {
            return false;
        };
        let Some(source) = ctx.entity_at(source_index).ok().flatten() else {
            return false;
        };
        let Some(snapshot) = ctx.entity_relationships(source) else {
            return false;
        };
        let Some(world) = ctx.entity_at(0).ok().flatten() else {
            return false;
        };
        if let Ok(message) = std::ffi::CString::new(format!(
            "RELATIONSHIP OBSERVED phase={phase:?} expected_target={target_index} snapshot={snapshot:?}\n"
        )) {
            ctx.server().print(&message);
        }
        if snapshot.entity != source
            || snapshot.aiment != Some(source)
            || snapshot.ground_entity != Some(world)
        {
            return false;
        }
        match phase.to_bytes() {
            b"live" if self.phase == 0 => {
                let Some(target) = ctx.entity_at(target_index).ok().flatten() else {
                    return false;
                };
                if snapshot.owner != Some(target) || snapshot.enemy != Some(target) {
                    return false;
                }
                self.source = Some(source);
                self.target = Some(target);
                self.phase = 1;
            }
            b"retired" if self.phase == 1 => {
                if self.source != Some(source)
                    || snapshot.owner.is_some()
                    || snapshot.enemy.is_some()
                    || !self
                        .target
                        .is_some_and(|target| !ctx.is_entity_current(target))
                {
                    return false;
                }
                self.phase = 2;
            }
            b"reused" if self.phase == 2 => {
                let Some(target) = ctx.entity_at(target_index).ok().flatten() else {
                    return false;
                };
                if self.source != Some(source)
                    || snapshot.owner != Some(target)
                    || snapshot.enemy != Some(target)
                    || self.target == Some(target)
                    || !self.target.is_some_and(|old| !ctx.is_entity_current(old))
                {
                    return false;
                }
                self.target = Some(target);
                self.phase = 3;
            }
            _ => return false,
        }
        true
    }
}
