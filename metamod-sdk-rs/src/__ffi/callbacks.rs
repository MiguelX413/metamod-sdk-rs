//! Native trampolines and immediate lifecycle bookkeeping.

use super::dispatch::{drain, hook, submit, synchronous};
use super::players::{player_slot, reset_players};
use super::state::{
    ACTIVATION, EVENTS, Event, INDEX_LOOKUP, SHUTDOWN, contained, registration_available,
    wrong_thread,
};
use crate::engine;
use crate::{ClientAdmission, ClientConnection, HookResult, KeyValueEvent, Plugin, PostHookResult};
use metamod_sys::raw::edict_t;
use metamod_sys::*;
use std::collections::hash_map::Entry;
use std::ffi::{c_char, c_int};
use std::ptr;

pub(super) unsafe extern "C" fn client_connect<P: Plugin>(
    _: *mut edict_t,
    name: *const c_char,
    address: *const c_char,
    reject: *mut c_char,
) -> c_int {
    let reason = synchronous::<P, _>(|plugin, context, _| {
        if reject.is_null() {
            return HookResult::Ignored;
        }
        // SAFETY: ClientConnect supplies C strings readable through their NUL
        // or the scan limit. Copy both before running any plugin code.
        let Some(name) = (unsafe { engine::copy_string(name, engine::MAX_ENGINE_STRING) }) else {
            return HookResult::Ignored;
        };
        // SAFETY: The address follows the same input contract. No callback has run.
        let Some(address) = (unsafe { engine::copy_string(address, engine::MAX_ENGINE_STRING) })
        else {
            return HookResult::Ignored;
        };
        match plugin.client_connect(context, &ClientConnection { name, address }) {
            ClientAdmission::Defer => HookResult::Ignored,
            ClientAdmission::Reject(reason) => HookResult::Supercede(Some(reason)),
        }
    });
    if let Some(reason) = reason {
        let bytes = reason.as_c_str().to_bytes_with_nul();
        // SAFETY: The host supplies a writable 128-byte buffer through the
        // entire hook, including native reentry. The owned reason validates
        // at most 127 payload bytes plus its NUL; no truncation occurs here.
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr().cast(), reject, bytes.len());
        }
        return 0;
    }
    // Metamod ignores this placeholder for MRES_IGNORED.
    1
}

pub(super) unsafe extern "C" fn lag_pre<P: Plugin>() -> c_int {
    c_int::from(synchronous::<P, _>(|plugin, context, _| {
        plugin.allow_lag_compensation(context)
    }))
}

pub(super) unsafe extern "C" fn lag_post<P: Plugin>() -> c_int {
    c_int::from(synchronous::<P, _>(|plugin, context, globals| {
        // SAFETY: Metamod supplies an int return slot for this specific post hook.
        // Copy before user code can reenter Metamod. A superceded call has no
        // original GameDLL result even if Metamod supplies a fallback slot.
        let original = unsafe {
            let slot = (*globals).orig_ret.cast::<c_int>();
            ((*globals).status != MRES_SUPERCEDE && !slot.is_null() && slot.is_aligned())
                .then(|| slot.read() != 0)
        };
        match plugin.allow_lag_compensation_post(context, original) {
            PostHookResult::Ignored => HookResult::Ignored,
            PostHookResult::Handled => HookResult::Handled,
            PostHookResult::Override(value) => HookResult::Override(value),
        }
    }))
}

pub(super) unsafe extern "C" fn server_command<P: Plugin>() {
    let _ = contained(|| {
        if wrong_thread() {
            return;
        }
        // Invalidate an outer snapshot even if registration or dispatch
        // has since been disabled. The command buffer may have changed.
        engine::invalidate_string_read();
        // Native commands do not own a Metamod hook-result slot.
        // SAFETY: The engine invoked this trampoline with live argument storage.
        if registration_available()
            && let Some(command) = unsafe { engine::read_command() }
        {
            submit::<P>(Event::Command(command));
        }
    });
}

pub(super) unsafe extern "C" fn key_value<P: Plugin>(
    _: *mut edict_t,
    data: *mut raw::KeyValueData,
) {
    hook(|| {
        if data.is_null() || !data.is_aligned() || !registration_available() {
            return;
        }
        // SAFETY: Metamod supplies initialized input fields and readable
        // strings for this invocation. Do not read or write fHandled.
        let (class, key, value) =
            unsafe { ((*data).szClassName, (*data).szKeyName, (*data).szValue) };
        let mut remaining = engine::MAX_ENGINE_STRING;
        let mut copy = |string| {
            // SAFETY: Each input is live until the first NUL or the cap.
            // Snapshot all strings before dispatch can call the engine.
            let owned = unsafe { engine::copy_string(string, remaining) }?;
            remaining -= owned.as_bytes_with_nul().len();
            Some(owned)
        };
        let Some(key) = copy(key) else { return };
        let Some(value) = copy(value) else { return };
        let classname = if class.is_null() {
            None
        } else {
            let Some(classname) = copy(class) else { return };
            Some(classname)
        };
        submit::<P>(Event::KeyValue(KeyValueEvent {
            classname,
            key,
            value,
        }));
    });
}

pub(super) unsafe extern "C" fn start_frame<P: Plugin>() {
    hook(|| submit::<P>(Event::Frame));
}

pub(super) unsafe extern "C" fn begin_map(_: *mut edict_t, _: c_int, _: c_int) {
    hook(|| {
        if SHUTDOWN.get() {
            return;
        }
        let (depth, cancelled) = ACTIVATION.get();
        ACTIVATION.set((depth.saturating_add(1), cancelled));
        crate::trace::invalidate_map();
        reset_players();
    });
}

pub(super) unsafe extern "C" fn server_activate<P: Plugin>(_: *mut edict_t, _: c_int, _: c_int) {
    hook(|| {
        let (depth, cancelled) = ACTIVATION.get();
        if depth == 0 {
            return;
        }
        ACTIVATION.set((depth - 1, depth > 1 && cancelled));
        if depth == 1 && !cancelled && !SHUTDOWN.get() {
            if INDEX_LOOKUP.get().is_some() {
                INDEX_LOOKUP.set(Some(true));
            }
            crate::trace::activate_map();
            submit::<P>(Event::Activate);
        }
    });
}

pub(super) unsafe extern "C" fn server_deactivate<P: Plugin>() {
    hook(|| {
        let (depth, _) = ACTIVATION.get();
        ACTIVATION.set((depth, depth != 0));
        crate::trace::invalidate_map();
        reset_players();
        submit::<P>(Event::Deactivate);
    });
}

pub(super) unsafe extern "C" fn player_join<P: Plugin>(edict: *mut edict_t) {
    hook(|| {
        crate::fake_client::observe_player_change();
        // SAFETY: The post hook supplies the live edict for this client.
        if let Some((slot, serial)) = unsafe { player_slot(edict) }
            && let Some(player) = EVENTS.with(|events| events.borrow_mut().join(slot, serial))
        {
            crate::fake_client::replaced(player.slot());
            submit::<P>(Event::Join(player));
        }
    });
}

pub(super) unsafe extern "C" fn player_disconnect<P: Plugin>(edict: *mut edict_t) {
    hook(|| {
        crate::fake_client::observe_player_change();
        // SAFETY: The pre hook supplies the edict being disconnected.
        if let Some((slot, serial)) = unsafe { player_slot(edict) }
            && let Some(player) = EVENTS.with(|events| {
                let mut events = events.borrow_mut();
                match events.players.entry(slot) {
                    Entry::Occupied(entry) if entry.get().1 == serial => Some(entry.remove().0),
                    _ => None,
                }
            })
        {
            crate::fake_client::disconnected(player);
            submit::<P>(Event::Disconnect(player));
        }
    });
}

pub(super) unsafe extern "C" fn game_shutdown<P: Plugin>() {
    hook(|| {
        if !SHUTDOWN.replace(true) {
            ACTIVATION.set((0, false));
            crate::trace::invalidate_map();
            reset_players();
            EVENTS.with(|events| events.borrow_mut().pending.clear());
            drain::<P>();
        }
    });
}
