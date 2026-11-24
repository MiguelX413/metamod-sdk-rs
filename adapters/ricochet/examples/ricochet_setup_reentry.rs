//! Test-only map reentry during allocation or after native setup, before session publication.
#![cfg(target_os = "linux")]
#![deny(unsafe_op_in_unsafe_fn)]

use metamod_sdk_rs::{__ffi, FakeClientError, prelude::*};
use std::{
    cell::Cell,
    ffi::{c_char, c_int},
    ptr,
};

type ServerCommand = unsafe extern "C" fn(*const c_char);
type ServerExecute = unsafe extern "C" fn();

thread_local! {
    static COMMANDS: Cell<Option<(ServerCommand, ServerExecute)>> = const { Cell::new(None) };
}

#[derive(Default)]
struct Probe {
    attempted: bool,
    incomplete: bool,
    activations: usize,
    deactivations: usize,
    joins: usize,
    finished: bool,
}

impl Plugin for Probe {
    const METADATA: Metadata = Metadata {
        name: c"SDK setup reentry probe",
        version: c"1",
        date: c"",
        author: c"metamod-sdk-rs",
        url: c"",
        log_tag: c"RICOREENTRY",
    };

    fn server_activate(&mut self, _: &mut Context<'_>) {
        self.activations += 1;
    }

    fn server_deactivate(&mut self, _: &mut Context<'_>) {
        self.deactivations += 1;
    }

    fn player_join(&mut self, _: &mut Context<'_>, _: PlayerId) {
        self.joins += 1;
    }

    fn start_frame(&mut self, ctx: &mut Context<'_>) {
        if self.finished {
            return;
        }
        let server = ctx.server();
        if !self.attempted {
            self.attempted = true;
            if std::env::var_os("METAMOD_FAKE_ALLOCATION_REENTRY").is_some() {
                let mut entered_setup = false;
                // SAFETY: Only the disposable GameDLL fixture enables this mode.
                // It tolerates disconnect before private data exists. The SDK
                // must stop before this closure can touch the retired slot.
                let result = unsafe {
                    server.create_fake_client(c"SDKReentryProbe", |_| {
                        entered_setup = true;
                        false
                    })
                };
                self.incomplete =
                    !entered_setup && matches!(result, Err(FakeClientError::SetupIncomplete));
                if self.incomplete {
                    server.print(c"FAKE_REENTRY EVENT adapter setup closure was not entered\n");
                }
            } else {
                let Some((queue, execute)) = COMMANDS.get() else {
                    self.finished = true;
                    return server.print(c"FAKE_REENTRY FAIL missing engine callbacks\n");
                };
                let mut executed = false;
                // SAFETY: This test runs only on the verified disposable stock
                // Ricochet snapshots. Complete native setup before executing a
                // fixed command; never address the edict after changelevel.
                let result = unsafe {
                    server.create_fake_client(c"SDKReentryProbe", |setup| {
                        if !(setup.connect(c"loopback")
                            && setup.put_in_server()
                            && setup.restore_fake_flag())
                        {
                            return false;
                        }
                        server.print(
                            c"FAKE_REENTRY EVENT native setup complete; entering changelevel\n",
                        );
                        queue(c"changelevel rc_deathmatch\n".as_ptr());
                        execute();
                        executed = true;
                        true
                    })
                };
                self.incomplete =
                    executed && matches!(result, Err(FakeClientError::SetupIncomplete));
            }
            if !self.incomplete || !slots_empty(ctx) || self.activations != 1 {
                self.finished = true;
                return server.print(c"FAKE_REENTRY FAIL setup returned unexpected evidence\n");
            }
            server.print(c"FAKE_REENTRY EVENT SetupIncomplete with no tracked PlayerId\n");
            return;
        }
        self.finished = true;
        if self.incomplete
            && self.activations == 2
            && self.deactivations == 1
            && self.joins == 0
            && slots_empty(ctx)
        {
            server.print(c"FAKE_REENTRY PASS no session published across reentrant map change\n");
        } else {
            server.print(c"FAKE_REENTRY FAIL missing lifecycle evidence or stale session\n");
        }
    }
}

fn slots_empty(ctx: &Context<'_>) -> bool {
    (1..=2).all(|index| {
        ctx.player_at(PlayerSlot::try_from(index).unwrap())
            .is_none()
    })
}

static mut INFO: __ffi::PluginInfo = __ffi::info::<Probe>();

#[unsafe(no_mangle)]
unsafe extern "C" fn GiveFnptrsToDll(
    engine: *mut __ffi::EngineFunctions,
    globals: *mut __ffi::EngineGlobals,
) {
    // SAFETY: Metamod supplies the initialized native table and globals.
    unsafe { __ffi::give_engine(engine, globals) };
    if !engine.is_null() && engine.is_aligned() {
        // SAFETY: Copy the two initialized slots during loader handoff.
        let callbacks = unsafe {
            ptr::addr_of!((*engine).pfnServerCommand)
                .read()
                .zip(ptr::addr_of!((*engine).pfnServerExecute).read())
        };
        COMMANDS.set(callbacks);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn Meta_Query(
    version: *const c_char,
    out: *mut *mut __ffi::PluginInfo,
    utilities: *mut __ffi::MetaUtilities,
) -> c_int {
    // SAFETY: Metamod supplies the query arguments.
    unsafe { __ffi::query(version, out, utilities, &raw mut INFO) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn Meta_Attach(
    now: __ffi::LoadTime,
    table: *mut __ffi::MetaFunctions,
    globals: *mut __ffi::MetaGlobals,
    game: *mut __ffi::GameDllFunctions,
) -> c_int {
    // SAFETY: Metamod supplies the attachment storage and GameDLL table.
    unsafe {
        let result = __ffi::attach::<Probe>(now, table, globals);
        if result != 0 {
            __ffi::capture_game_dll(game);
        }
        result
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn Meta_Detach(_: __ffi::LoadTime, _: __ffi::UnloadReason) -> c_int {
    0
}
