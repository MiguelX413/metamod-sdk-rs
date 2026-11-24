//! Server-thread state, independent TLS cells, and panic containment.

use super::callbacks::server_command;
use crate::engine::Engine;
use crate::{Command, Context, KeyValueEvent, PlayerId, PlayerSlot, Plugin};
use metamod_sys::*;
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::mem::ManuallyDrop;
use std::num::NonZeroU64;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::OnceLock;
use std::thread;
use std::thread::ThreadId;

#[derive(Clone, Copy)]
pub(super) struct Services {
    pub(super) id: *mut PluginInfo,
    pub(super) log: Log,
    pub(super) console: Log,
    pub(super) user_message: raw::MetamodGetUserMsgID,
}

#[derive(Clone, Copy)]
pub(super) enum State {
    Unqueried,
    Queried(Services),
    Attaching,
    Attached {
        services: Services,
        globals: *mut MetaGlobals,
        disabled: bool,
    },
}

pub(super) static OWNER: OnceLock<ThreadId> = OnceLock::new();

pub(super) enum Event {
    Frame,
    Activate,
    Deactivate,
    Join(PlayerId),
    Disconnect(PlayerId),
    Command(Command),
    KeyValue(KeyValueEvent),
}

// Bound both the queue and the total work, even if each callback only
// enqueues one successor. The budget counts the root notification too.
pub(super) const MAX_EVENTS_PER_DISPATCH: usize = 4096;

pub(super) struct Events {
    pub(super) players: HashMap<PlayerSlot, (PlayerId, i32)>,
    pub(super) serial: u64,
    pub(super) pending: VecDeque<Event>,
    pub(super) submitted: usize,
}

impl Events {
    pub(super) fn join(&mut self, slot: PlayerSlot, edict_serial: i32) -> Option<PlayerId> {
        self.players.remove(&slot);
        let serial = NonZeroU64::new(self.serial.checked_add(1)?)?;
        self.serial = serial.get();
        let player = PlayerId::new(slot, serial);
        self.players.insert(slot, (player, edict_serial));
        Some(player)
    }
}

thread_local! {
    pub(super) static STATE: Cell<State> = const { Cell::new(State::Unqueried) };
    pub(super) static ENGINE: Cell<Engine> = Cell::new(Engine::default());
    // Only GameShutdown may extract and drop an attached instance. TLS
    // teardown can occur after the engine has shut down.
    pub(super) static INSTANCE: RefCell<Option<ManuallyDrop<Box<dyn Any>>>> = const { RefCell::new(None) };
    pub(super) static EVENTS: RefCell<Events> = RefCell::new(Events {
        players: HashMap::new(), serial: 0, pending: VecDeque::new(), submitted: 0,
    });
    pub(super) static RUNNING: Cell<bool> = const { Cell::new(false) };
    pub(super) static INDEX_LOOKUP: Cell<Option<bool>> = const { Cell::new(None) };
    // Count paired activation hooks so an inner post hook cannot expose
    // an arena while the outer GameDLL activation is still running.
    pub(super) static ACTIVATION: Cell<(usize, bool)> = const { Cell::new((0, false)) };
    pub(super) static SHUTDOWN: Cell<bool> = const { Cell::new(false) };
}

pub(super) fn wrong_thread() -> bool {
    OWNER
        .get()
        .is_some_and(|owner| *owner != thread::current().id())
}

pub(crate) fn engine_available() -> bool {
    !wrong_thread()
        && SHUTDOWN
            .try_with(|shutdown| !shutdown.get())
            .unwrap_or(false)
}

pub(crate) fn engine() -> Option<Engine> {
    engine_available().then(|| ENGINE.get())
}

pub(crate) fn registration_available() -> bool {
    engine_available()
        && matches!(
            STATE.get(),
            State::Attached {
                disabled: false,
                ..
            }
        )
}

pub(super) fn contained<T>(callback: impl FnOnce() -> T) -> Option<T> {
    match catch_unwind(AssertUnwindSafe(callback)) {
        Ok(value) => Some(value),
        Err(payload) => {
            // The original unwind has ended. Release payload-owned resources
            // even on retryable attach failures. If Drop panics, retain only
            // the secondary payload rather than risk an unbounded panic chain.
            if let Err(secondary) = catch_unwind(AssertUnwindSafe(|| drop(payload))) {
                #[cfg(all(test, metamod_lsan))]
                crate::ignore_intentionally_retained(secondary.as_ref());
                std::mem::forget(secondary);
            }
            None
        }
    }
}

pub(super) fn make_context<P: Plugin>(scope: &(), services: Services) -> Context<'_> {
    Context::new(
        scope,
        services.id,
        services.log,
        services.console,
        services.user_message,
        ENGINE.get().print,
        server_command::<P>,
    )
}
