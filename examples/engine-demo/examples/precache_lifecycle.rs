//! Server-only precaching assertions for isolated live GoldSrc runs.
use metamod_sdk_rs::prelude::*;

const MODEL: &ResourcePath = ResourcePath::new(c"sprites/laserbeam.spr").unwrap();
const SOUND: &ResourcePath = ResourcePath::new(c"common/null.wav").unwrap();

#[derive(Default)]
struct Precaching {
    registrations: usize,
    activations: usize,
    announced: bool,
    command_attempted: bool,
    failed: bool,
    current: Option<(ModelIndex, SoundIndex)>,
    previous: Option<(ModelIndex, SoundIndex)>,
}

#[metamod_plugin(
    name = "SDK Precache Lifecycle Probe",
    version = "1",
    author = "metamod-sdk-rs contributors",
    log_tag = "PRECACHEPROBE"
)]
impl Plugin for Precaching {
    const PRECACHE: bool = true;

    fn precache(&mut self, resources: &mut Precache<'_>) {
        self.registrations += 1;
        let result = (
            resources.model(MODEL),
            resources.sound(SOUND),
            resources.model(MODEL),
            resources.sound(SOUND),
        );
        match result {
            (Ok(model), Ok(sound), Ok(model_again), Ok(sound_again))
                if model == model_again && sound == sound_again =>
            {
                self.current = Some((model, sound));
                println!(
                    "PRECACHE REGISTER PASS visit={} model={} sound={} duplicates=equal",
                    self.registrations,
                    model.get(),
                    sound.get()
                );
            }
            observed => self.fail(format!("registration {observed:?}")),
        }
    }

    fn server_activate(&mut self, ctx: &mut Context<'_>) {
        self.activations += 1;
        self.announced = false;
        let Some((model, sound)) = self.current else {
            self.fail("no registered indices");
            return;
        };
        if self.registrations != self.activations
            || !ctx.is_model_current(model)
            || !ctx.is_sound_current(sound)
        {
            self.fail("activation identity");
            return;
        }
        if let Some((old_model, old_sound)) = self.previous {
            if ctx.is_model_current(old_model)
                || ctx.is_sound_current(old_sound)
                || old_model == model
                || old_sound == sound
            {
                self.fail("old indices survived the map change");
                return;
            }
            println!(
                "PRECACHE STALE PASS old_model={} new_model={} old_sound={} new_sound={}",
                old_model.get(),
                model.get(),
                old_sound.get(),
                sound.get()
            );
        }
        let expected = match self.activations {
            1 => c"rc_arena",
            2 => c"rc_deathmatch",
            _ => {
                self.fail("unexpected map visit");
                return;
            }
        };
        if ctx
            .server()
            .state()
            .and_then(|state| state.map_name)
            .as_deref()
            != Some(expected)
        {
            self.fail("unexpected map name");
            return;
        }
        println!(
            "PRECACHE ACTIVATE PASS visit={} model={} sound={}",
            self.activations,
            model.get(),
            sound.get()
        );
    }

    fn server_deactivate(&mut self, ctx: &mut Context<'_>) {
        let Some((model, sound)) = self.current else {
            self.fail("deactivation without indices");
            return;
        };
        if ctx.is_model_current(model) || ctx.is_sound_current(sound) {
            self.fail("indices stayed current during deactivation");
            return;
        }
        self.previous = self.current;
        println!("PRECACHE DEACTIVATE PASS visit={}", self.activations);
    }

    fn start_frame(&mut self, ctx: &mut Context<'_>) {
        if !self.command_attempted {
            self.command_attempted = true;
            if let Err(error) = ctx
                .server()
                .register_command(console_name!(c"sdk_precache_status"))
            {
                self.fail(format!("command registration: {error}"));
            }
        }
        if !self.failed && !self.announced {
            self.announced = true;
            println!("PRECACHE READY visit={}", self.activations);
        }
    }

    fn server_command(&mut self, ctx: &mut Context<'_>, _: &Command) {
        if self.failed
            || self.registrations != self.activations
            || !self.current.is_some_and(|(model, sound)| {
                ctx.is_model_current(model) && ctx.is_sound_current(sound)
            })
        {
            self.fail("status identity");
            return;
        }
        println!("PRECACHE STATUS PASS visit={}", self.activations);
    }
}

impl Precaching {
    fn fail(&mut self, reason: impl std::fmt::Display) {
        self.failed = true;
        println!("PRECACHE FAIL {reason}");
    }
}
