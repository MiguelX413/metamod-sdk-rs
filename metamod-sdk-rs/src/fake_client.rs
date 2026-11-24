use crate::{__ffi, PlayerId, PlayerSlot, PlayerUserId, Server, trace};
use metamod_sys::{DllFunctions, GameDllFunctions, raw::edict_t};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ffi::{CStr, CString},
    ptr,
    rc::Rc,
};

/// An engine-owned fake-client session. Dropping this handle leaves the session alone.
#[derive(Clone, Debug)]
pub struct FakeClient {
    player: PlayerId,
    state: Rc<Cell<FakeClientState>>,
}

impl FakeClient {
    /// Return the original tracked session ID. Revalidate it before use;
    /// retaining or cloning this handle does not keep the session alive.
    pub fn player(&self) -> PlayerId {
        self.player
    }

    /// Return the last lifecycle state observed by the SDK. This reads cached
    /// bookkeeping and does not query the engine or force removal confirmation.
    pub fn state(&self) -> FakeClientState {
        self.state.get()
    }
}

/// Lifecycle state shared by clones of a [`FakeClient`] handle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FakeClientState {
    /// Setup completed and no removal has been queued by this handle.
    /// Use session validation for a fresh engine observation.
    Active,
    /// A kick command was queued; native removal has not yet been confirmed.
    RemovalQueued,
    /// A disconnect and subsequent native identity check confirmed removal.
    Removed,
    /// Tracking was invalidated, for example by map retirement or slot replacement.
    /// This does not confirm that a queued removal completed.
    Invalidated,
}

/// Fake-client creation or queued removal could not complete as requested.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FakeClientError {
    /// The requested name is invalid for the game-specific adapter.
    /// The core allocator currently leaves name validation to adapters.
    InvalidName,
    /// Required engine/GameDLL callbacks or lifecycle state are unavailable,
    /// or another creation is already in progress.
    Unavailable,
    /// The native allocation callback returned null without an observed lifecycle change.
    AllocationFailed,
    /// Allocation may have succeeded. The SDK cannot safely roll it back.
    SetupIncomplete,
    /// The handle is no longer an active tracked client eligible for removal.
    /// A removal already queued through this handle is also not active.
    NotActive,
}

#[derive(Clone)]
struct Session {
    handle: FakeClient,
    identity: Identity,
    disconnect_seen: bool,
}

#[derive(Clone, Copy)]
struct Identity {
    map: u64,
    slot: PlayerSlot,
    serial: i32,
    user: PlayerUserId,
}

thread_local! {
    static GAME: Cell<DllFunctions> = Cell::new(DllFunctions::default());
    static CLIENTS: RefCell<HashMap<PlayerSlot, Session>> = RefCell::new(HashMap::new());
    static CLIENT_REVISION: Cell<u64> = const { Cell::new(0) };
    static CREATING: Cell<bool> = const { Cell::new(false) };
    static POLLING: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn observe_player_change() {
    CLIENT_REVISION.update(|revision| revision.saturating_add(1));
}

fn stable_revision(revision: u64) -> bool {
    revision != u64::MAX && CLIENT_REVISION.get() == revision
}

pub(crate) unsafe fn capture_game_dll(game: *mut GameDllFunctions) {
    if game.is_null() || !game.is_aligned() {
        return;
    }
    // SAFETY: Metamod supplies an initialized pointer to the original table.
    let table = unsafe { (*game).dllapi_table };
    if !table.is_null() && table.is_aligned() {
        // SAFETY: The host's two initialized callbacks are copied without borrowing its table.
        GAME.set(unsafe {
            DllFunctions {
                pfnClientConnect: ptr::addr_of!((*table).pfnClientConnect).read(),
                pfnClientPutInServer: ptr::addr_of!((*table).pfnClientPutInServer).read(),
                ..Default::default()
            }
        });
    }
}

/// Short-lived access for an adapter to initialize the GameDLL player.
pub struct FakeClientSetup<'a> {
    identity: Identity,
    revision: u64,
    edict: *mut edict_t,
    game: DllFunctions,
    name: &'a CStr,
}

impl FakeClientSetup<'_> {
    /// Invoke the original GameDLL admission function, bypassing Metamod dispatch.
    ///
    /// # Safety
    /// The active GameDLL must support this initialization step for a newly allocated fake client.
    pub unsafe fn connect(&mut self, address: &CStr) -> bool {
        if !self.is_current() {
            return false;
        }
        let Some(connect) = self.game.pfnClientConnect else {
            return false;
        };
        let mut reason = [0; 128];
        // SAFETY: Identity was revalidated, and the input and output buffers live through the call.
        let accepted = unsafe {
            connect(
                self.edict,
                self.name.as_ptr(),
                address.as_ptr(),
                reason.as_mut_ptr(),
            ) != 0
        };
        accepted && self.is_current()
    }

    /// Invoke the original GameDLL put-in-server function.
    ///
    /// # Safety
    /// The adapter must establish that the GameDLL is ready for this step.
    pub unsafe fn put_in_server(&mut self) -> bool {
        if !self.is_current() {
            return false;
        }
        let Some(put) = self.game.pfnClientPutInServer else {
            return false;
        };
        // SAFETY: The slot was revalidated with no intervening native call.
        unsafe { put(self.edict) };
        self.is_current()
    }

    /// Set the fake-client marker after a supported GameDLL spawn clears it.
    ///
    /// # Safety
    /// Restoring this flag must be valid for the active GameDLL.
    pub unsafe fn restore_fake_flag(&mut self) -> bool {
        if !self.is_current() {
            return false;
        }
        // SAFETY: Revalidation established an initialized edict and flags.
        unsafe { (*self.edict).v.flags |= metamod_sys::raw::FL_FAKECLIENT as i32 };
        true
    }

    fn is_current(&self) -> bool {
        stable_revision(self.revision) && self.identity.resolve() == Some(self.edict)
    }
}

impl Identity {
    fn resolve(self) -> Option<*mut edict_t> {
        let revision = CLIENT_REVISION.get();
        let engine = __ffi::engine()?;
        if trace::active_map() != Some(self.map) {
            return None;
        }
        let edict = trace::tracked_entity(self.slot.get(), self.serial)?.resolve(engine)?;
        if !stable_revision(revision) {
            return None;
        }
        // SAFETY: The validated slot is live and the callback takes a readable edict.
        let user = unsafe { engine.userid?(edict) };
        // The getter can reenter. Check the epoch before accessing the slot again.
        (stable_revision(revision) && trace::active_map() == Some(self.map) && user == self.user.get()
            // SAFETY: Within the same map epoch the host keeps slot identity readable.
            && unsafe { trace::live_serial(edict) } == Some(self.serial))
        .then_some(edict)
    }
}

impl Server<'_, '_> {
    /// Allocate and initialize a fake client with a GameDLL-specific adapter.
    ///
    /// # Safety
    /// The operator must establish that the active engine, Metamod and GameDLL
    /// support the adapter's setup, callback and teardown contracts. In particular,
    /// the host must return a live slot on success, provide stable positive user
    /// IDs for occupied slots and copy queued command text. User IDs must not be
    /// reassigned while a queued kick can target them. The user-ID getter
    /// must return a different value only once the old occupant no longer owns
    /// the slot. The adapter must provide a valid engine name and return true
    /// only after complete GameDLL setup. Dropping an incomplete
    /// allocation is GameDLL-specific and the SDK never attempts it.
    pub unsafe fn create_fake_client(
        &self,
        name: &CStr,
        setup: impl FnOnce(&mut FakeClientSetup<'_>) -> bool,
    ) -> Result<FakeClient, FakeClientError> {
        use FakeClientError::*;
        if !__ffi::registration_available() {
            return Err(Unavailable);
        }
        let map = trace::active_map().ok_or(Unavailable)?;
        let engine = __ffi::engine().ok_or(Unavailable)?;
        let create = engine.create_fake_client.ok_or(Unavailable)?;
        let userid = engine.userid.ok_or(Unavailable)?;
        let game = GAME.get();
        if game.pfnClientConnect.is_none()
            || game.pfnClientPutInServer.is_none()
            || engine.index.is_none()
            || engine.entity.is_none()
        {
            return Err(Unavailable);
        }
        if CREATING.replace(true) {
            return Err(Unavailable);
        }
        struct CreateGuard;
        impl Drop for CreateGuard {
            fn drop(&mut self) {
                CREATING.set(false);
            }
        }
        let _guard = CreateGuard;
        let revision = CLIENT_REVISION.get();
        // SAFETY: The caller guarantees that allocation is supported in this host.
        let edict = unsafe { create(name.as_ptr()) };
        if trace::active_map() != Some(map) || !stable_revision(revision) {
            return Err(SetupIncomplete);
        }
        if edict.is_null() {
            return Err(AllocationFailed);
        }
        // SAFETY: Allocation supplies a live slot in this still-active map.
        let (slot, serial) = unsafe { __ffi::player_slot(edict) }.ok_or(SetupIncomplete)?;
        if trace::active_map() != Some(map)
            || !stable_revision(revision)
            || trace::tracked_entity(slot.get(), serial).is_none()
        {
            return Err(SetupIncomplete);
        }
        // SAFETY: player_slot revalidated the live allocation.
        let user = PlayerUserId::new(unsafe { userid(edict) }).ok_or(SetupIncomplete)?;
        if !stable_revision(revision) {
            return Err(SetupIncomplete);
        }
        let identity = Identity {
            map,
            slot,
            serial,
            user,
        };
        let mut adapter = FakeClientSetup {
            identity,
            revision,
            edict,
            game,
            name,
        };
        if !adapter.is_current()
            || !setup(&mut adapter)
            || !stable_revision(revision)
            || !adapter.is_current()
        {
            return Err(SetupIncomplete);
        }
        let player = __ffi::track_fake_client(slot, serial).ok_or(SetupIncomplete)?;
        let handle = FakeClient {
            player,
            state: Rc::new(Cell::new(FakeClientState::Active)),
        };
        CLIENTS.with(|clients| {
            if let Some(previous) = clients.borrow_mut().insert(
                slot,
                Session {
                    handle: handle.clone(),
                    identity,
                    disconnect_seen: false,
                },
            ) {
                previous.handle.state.set(FakeClientState::Invalidated);
            }
        });
        Ok(handle)
    }

    /// Queue teardown for this session. Completion is reported by `FakeClient::state`.
    ///
    /// Success only queues a command; it does not mean the client has left.
    /// A later disconnect plus native identity confirmation produces
    /// [`FakeClientState::Removed`]. Map changes and replacement can instead
    /// produce [`FakeClientState::Invalidated`]. Dropping the handle does not
    /// request removal or cancel a queued command.
    ///
    /// ```
    /// use metamod_sdk_rs::{Context, FakeClient, FakeClientError, FakeClientState};
    ///
    /// fn request_removal(ctx: &Context<'_>, client: &FakeClient) -> Result<(), FakeClientError> {
    ///     if client.state() == FakeClientState::Active {
    ///         ctx.server().request_fake_client_removal(client)?;
    ///     }
    ///     // Inspect state on a later callback to observe completion or invalidation.
    ///     Ok(())
    /// }
    /// ```
    pub fn request_fake_client_removal(&self, client: &FakeClient) -> Result<(), FakeClientError> {
        use FakeClientError::*;
        let queue = __ffi::engine()
            .and_then(|engine| engine.queue_server_command)
            .ok_or(Unavailable)?;
        let slot = client.player.slot();
        let get_session = || {
            CLIENTS
                .with(|clients| clients.borrow().get(&slot).cloned())
                .filter(|session| session.handle.player == client.player)
        };
        let session = get_session().ok_or(NotActive)?;
        if client.state() != FakeClientState::Active
            || session.identity.resolve().is_none()
            || __ffi::player_at(slot) != Some(client.player)
            || get_session().is_none()
        {
            return Err(NotActive);
        }
        let Ok(command) = CString::new(format!("kick #{}\n", session.identity.user)) else {
            return Err(Unavailable);
        };
        client.state.set(FakeClientState::RemovalQueued);
        // SAFETY: The opted-in host copies synchronous command inputs. The
        // numeric engine user ID identifies the occupant beyond this call.
        unsafe { queue(command.as_ptr()) };
        Ok(())
    }
}

pub(crate) fn invalidate() {
    observe_player_change();
    CLIENTS.with(|clients| {
        for (_, session) in clients.borrow_mut().drain() {
            session.handle.state.set(FakeClientState::Invalidated);
        }
    });
}

pub(crate) fn replaced(slot: PlayerSlot) {
    CLIENTS.with(|clients| {
        if let Some(session) = clients.borrow_mut().remove(&slot) {
            session.handle.state.set(FakeClientState::Invalidated);
        }
    });
}

pub(crate) fn disconnected(player: PlayerId) {
    CLIENTS.with(|clients| {
        if let Some(session) = clients.borrow_mut().get_mut(&player.slot())
            && session.handle.player == player
        {
            session.disconnect_seen = true;
        }
    });
}

pub(crate) fn poll_removals() {
    if POLLING.replace(true) {
        return;
    }
    struct PollGuard;
    impl Drop for PollGuard {
        fn drop(&mut self) {
            POLLING.set(false);
        }
    }
    let _guard = PollGuard;
    let pending: Vec<_> = CLIENTS.with(|clients| {
        clients
            .borrow()
            .values()
            .filter(|session| session.disconnect_seen)
            .cloned()
            .collect()
    });
    for session in pending {
        let identity = session.identity;
        let revision = CLIENT_REVISION.get();
        let Some(engine) = __ffi::engine() else {
            return;
        };
        let (Some(lookup), Some(userid)) = (engine.entity, engine.userid) else {
            return;
        };
        if trace::active_map() != Some(identity.map) {
            continue;
        }
        // SAFETY: The previously validated slot is still in this map.
        let edict = unsafe { lookup(identity.slot.get()) };
        if !stable_revision(revision)
            || trace::active_map() != Some(identity.map)
            || edict.is_null()
            || !edict.is_aligned()
        {
            continue;
        }
        // SAFETY: The host returned a current-map slot edict.
        let user = unsafe { userid(edict) };
        if stable_revision(revision)
            && trace::active_map() == Some(identity.map)
            && user != identity.user.get()
        {
            CLIENTS.with(|clients| {
                let mut clients = clients.borrow_mut();
                if clients
                    .get(&identity.slot)
                    .is_some_and(|current| current.handle.player == session.handle.player)
                {
                    clients.remove(&identity.slot);
                    session.handle.state.set(FakeClientState::Removed);
                }
            });
        }
    }
}
