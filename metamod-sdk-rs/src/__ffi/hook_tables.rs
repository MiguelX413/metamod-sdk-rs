//! Version negotiation and registration of pre, post, and NewDLL hooks.

use super::callbacks::{
    begin_map, client_connect, game_shutdown, key_value, lag_post, lag_pre, player_disconnect,
    player_join, server_activate, server_deactivate, spawn, start_frame,
};
use super::state::wrong_thread;
use crate::Plugin;
use metamod_sys::raw::NEW_DLL_FUNCTIONS;
use metamod_sys::*;
use std::ffi::c_int;

/// # Safety
/// Non-null aligned pointers must designate a writable output table and
/// an initialized writable version integer respectively.
unsafe fn write_hooks<T>(table: *mut T, version: *mut c_int, expected: c_int, hooks: T) -> c_int {
    if wrong_thread()
        || table.is_null()
        || version.is_null()
        || !table.is_aligned()
        || !version.is_aligned()
    {
        return 0;
    }
    // SAFETY: Metamod provides valid version storage and a writable table.
    unsafe {
        if version.read() != expected {
            version.write(expected);
            return 0;
        }
        table.write(hooks);
    }
    1
}

pub(super) unsafe extern "C" fn entity_api<P: Plugin>(
    table: *mut DllFunctions,
    version: *mut c_int,
) -> c_int {
    // SAFETY: Metamod supplies the API negotiation arguments.
    unsafe {
        write_hooks(
            table,
            version,
            ENTITY_INTERFACE,
            DllFunctions {
                pfnSpawn: P::PRECACHE.then_some(spawn::<P>),
                pfnClientConnect: P::CLIENT_CONNECT_PRE.then_some(client_connect::<P>),
                pfnStartFrame: Some(start_frame::<P>),
                pfnKeyValue: P::KEYVALUE_PRE.then_some(key_value::<P>),
                pfnAllowLagCompensation: P::LAG_COMPENSATION_PRE.then_some(lag_pre::<P>),
                pfnServerActivate: Some(begin_map),
                pfnServerDeactivate: Some(server_deactivate::<P>),
                pfnClientDisconnect: Some(player_disconnect::<P>),
                ..Default::default()
            },
        )
    }
}

pub(super) unsafe extern "C" fn entity_api_post<P: Plugin>(
    table: *mut DllFunctions,
    version: *mut c_int,
) -> c_int {
    // SAFETY: Metamod supplies the API negotiation arguments.
    unsafe {
        write_hooks(
            table,
            version,
            ENTITY_INTERFACE,
            DllFunctions {
                pfnServerActivate: Some(server_activate::<P>),
                pfnAllowLagCompensation: P::LAG_COMPENSATION_POST.then_some(lag_post::<P>),
                pfnClientPutInServer: Some(player_join::<P>),
                ..Default::default()
            },
        )
    }
}

pub(super) unsafe extern "C" fn new_api<P: Plugin>(
    table: *mut NEW_DLL_FUNCTIONS,
    version: *mut c_int,
) -> c_int {
    // SAFETY: Metamod supplies the API negotiation arguments.
    unsafe {
        write_hooks(
            table,
            version,
            raw::NEW_DLL_FUNCTIONS_VERSION as c_int,
            NEW_DLL_FUNCTIONS {
                pfnGameShutdown: Some(game_shutdown::<P>),
                ..Default::default()
            },
        )
    }
}
