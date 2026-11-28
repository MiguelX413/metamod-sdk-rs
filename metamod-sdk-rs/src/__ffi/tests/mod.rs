use super::*;
use super::{callbacks::*, dispatch::*, hook_tables::*, players::*, state::*};
use crate::Metadata;
use crate::{
    ClientAdmission, ClientConnection, Context, HookResult, PlayerSlot, Plugin,
    engine::{self, Engine},
};
use raw::edict_t;
use std::{
    cell::{Cell, RefCell},
    ffi::{CStr, c_char, c_int},
    ptr, thread,
};

const METADATA: Metadata = Metadata {
    name: c"test",
    version: c"",
    date: c"",
    author: c"",
    url: c"",
    log_tag: c"",
};

fn queried() {
    STATE.set(State::Queried(Services {
        id: ptr::null_mut(),
        log: None,
        console: None,
        user_message: None,
    }));
}

// Keep test names stable for the memory-evidence importer and Miri selections.
include!("players.rs");
include!("console.rs");
include!("messages.rs");
include!("fake_clients.rs");
include!("hooks.rs");
include!("dispatch.rs");
include!("lifecycle.rs");

include!("server_state.rs");
include!("relationships.rs");
include!("resources.rs");
