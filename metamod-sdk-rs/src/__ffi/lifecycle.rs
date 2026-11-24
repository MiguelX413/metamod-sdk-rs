//! Engine handoff, Metamod negotiation, and startup attachment.

use super::hook_tables::{entity_api, entity_api_post, new_api};
use super::state::{
    ENGINE, INSTANCE, OWNER, STATE, Services, State, contained, make_context, wrong_thread,
};
use crate::Plugin;
use crate::engine;
use crate::engine::Engine;
use metamod_sys::*;
use std::ffi::{CStr, c_char, c_int};
use std::mem::ManuallyDrop;
use std::ptr;
use std::thread;

/// # Safety
/// A non-null table must have aligned, readable, initialized callback
/// slots. The first valid handoff binds the attachment to this server
/// thread. The callbacks must remain callable on it through shutdown.
/// Non-null globals must be aligned and readable through shutdown.
pub unsafe fn give_engine(table: *mut EngineFunctions, globals: *mut EngineGlobals) {
    let _ = contained(|| {
        if table.is_null() || !table.is_aligned() || !globals.is_aligned() {
            return;
        }
        let current = thread::current().id();
        if OWNER.get_or_init(|| current) != &current
            || !STATE.with(|state| matches!(state.get(), State::Unqueried))
        {
            return;
        }
        // SAFETY: The loader supplies these initialized slots. Copy them
        // without borrowing the table, which may be shared by the loader.
        let engine = unsafe { Engine::read(table) };
        ENGINE.set(engine);
        // SAFETY: The loader supplies the lifetime of the globals.
        unsafe { crate::trace::set_globals(globals) };
    });
}

pub const fn info<P: Plugin>() -> PluginInfo {
    let metadata = P::METADATA;
    PluginInfo {
        ifvers: META_INTERFACE.as_ptr(),
        name: metadata.name.as_ptr(),
        version: metadata.version.as_ptr(),
        date: metadata.date.as_ptr(),
        author: metadata.author.as_ptr(),
        url: metadata.url.as_ptr(),
        logtag: metadata.log_tag.as_ptr(),
        loadable: PT_STARTUP,
        unloadable: PT_NEVER,
    }
}

fn interface_version(version: &CStr) -> Option<(u32, u32)> {
    let (major, minor) = version.to_str().ok()?.split_once(':')?;
    let number = |part: &str| {
        part.bytes()
            .all(|byte| byte.is_ascii_digit())
            .then(|| part.parse::<u32>().ok())
            .flatten()
    };
    Some((number(major)?, number(minor)?))
}

fn compatible(version: &CStr) -> bool {
    match (
        interface_version(version),
        interface_version(META_INTERFACE),
    ) {
        (Some((major, minor)), Some((required_major, required_minor))) => {
            major == required_major && minor >= required_minor
        }
        _ => false,
    }
}

/// # Safety
/// The version must be readable up to its NUL or 64 bytes. The utility
/// slots must be aligned and initialized, the output aligned and writable,
/// and the metadata must remain live for the module lifetime. Its strings
/// must be valid for host reads. Non-null utility callbacks must be valid
/// for the plugin ID and callable through shutdown. Metamod must invoke
/// query after the engine handoff and on the same server thread.
pub unsafe fn query(
    version: *const c_char,
    out: *mut *mut PluginInfo,
    utilities: *mut MetaUtilities,
    id: *mut PluginInfo,
) -> c_int {
    contained(|| {
        if OWNER.get() != Some(&thread::current().id())
            || version.is_null()
            || out.is_null()
            || !out.is_aligned()
            || utilities.is_null()
            || !utilities.is_aligned()
            || id.is_null()
            || !id.is_aligned()
        {
            return 0;
        }
        // SAFETY: Validity follows from the loader contract above.
        let Some(version) = (unsafe { engine::copy_string(version, 64) }) else {
            return 0;
        };
        if !compatible(&version) {
            return 0;
        }
        STATE.with(|state| {
            if !matches!(state.get(), State::Unqueried | State::Queried(_)) {
                return 0;
            }
            // SAFETY: The utility table is readable and the output is writable.
            let log = unsafe { ptr::addr_of!((*utilities).pfnLogMessage).read() };
            let console = unsafe { ptr::addr_of!((*utilities).pfnLogConsole).read() };
            // SAFETY: The negotiated utility table includes this initialized slot.
            let user_message = unsafe { ptr::addr_of!((*utilities).pfnGetUserMsgID).read() };
            // SAFETY: This is the loader's writable output slot.
            unsafe { out.write(id) };
            state.set(State::Queried(Services {
                id,
                log,
                console,
                user_message,
            }));
            1
        })
    })
    .unwrap_or(0)
}

/// # Safety
/// Called on the server thread that queried the plugin. Metamod must supply
/// an aligned writable table that stays valid through initialization and
/// any native reentry it causes. Metamod must keep the aligned globals valid
/// until each hook returns, including when a nested hook signals shutdown.
pub unsafe fn attach<P: Plugin>(
    now: LoadTime,
    table: *mut MetaFunctions,
    globals: *mut MetaGlobals,
) -> c_int {
    if wrong_thread()
        || now != PT_STARTUP
        || table.is_null()
        || globals.is_null()
        || !table.is_aligned()
        || !globals.is_aligned()
    {
        return 0;
    }
    contained(|| {
        STATE.with(|state| {
            let State::Queried(services) = state.get() else {
                return 0;
            };
            state.set(State::Attaching);
            let Some(mut plugin) = contained(|| Box::new(P::default())) else {
                state.set(State::Queried(services));
                return 0;
            };
            let scope = ();
            let mut context = make_context::<P>(&scope, services);
            if contained(|| plugin.initialize(&mut context)) != Some(true) {
                // Drop separately so a destructor panic cannot collide with
                // an unwind from initialization or escape into Metamod.
                let _ = contained(|| drop(plugin));
                state.set(State::Queried(services));
                return 0;
            }

            INSTANCE.with(|instance| instance.replace(Some(ManuallyDrop::new(plugin))));
            // SAFETY: Metamod provided a full writable output table.
            unsafe {
                table.write(MetaFunctions {
                    pfnGetEntityAPI2: Some(entity_api::<P>),
                    pfnGetEntityAPI2_Post: Some(entity_api_post::<P>),
                    pfnGetNewDLLFunctions: Some(new_api::<P>),
                    ..Default::default()
                });
            }
            state.set(State::Attached {
                services,
                globals,
                disabled: false,
            });
            1
        })
    })
    .unwrap_or(0)
}

/// # Safety
/// The loader supplies initialized GameDLL function slots valid through shutdown.
pub unsafe fn capture_game_dll(game: *mut GameDllFunctions) {
    // SAFETY: The export receives the native table from Metamod.
    unsafe { crate::fake_client::capture_game_dll(game) };
}
