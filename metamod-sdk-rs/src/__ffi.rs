//! Implementation details for the export macro.

pub use metamod_sys::*;

use crate::{
    ClientAdmission, ClientConnection, Command, Context, HookResult, KeyValueEvent, PlayerId,
    PlayerSlot, Plugin, PostHookResult,
    engine::{self, Engine},
};
use raw::{NEW_DLL_FUNCTIONS, edict_t};
use std::{
    any::Any,
    cell::{Cell, RefCell},
    collections::{HashMap, VecDeque, hash_map::Entry},
    ffi::{CStr, c_char, c_int},
    io::{self, Write},
    mem::ManuallyDrop,
    num::NonZeroU64,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
    sync::OnceLock,
    thread::{self, ThreadId},
};

#[derive(Clone, Copy)]
struct Services {
    id: *mut PluginInfo,
    log: Log,
    console: Log,
}

#[derive(Clone, Copy)]
enum State {
    Unqueried,
    Queried(Services),
    Attaching,
    Attached {
        services: Services,
        globals: *mut MetaGlobals,
        disabled: bool,
    },
}

static OWNER: OnceLock<ThreadId> = OnceLock::new();

enum Event {
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
const MAX_EVENTS_PER_DISPATCH: usize = 4096;

struct Events {
    players: HashMap<PlayerSlot, (PlayerId, i32)>,
    serial: u64,
    pending: VecDeque<Event>,
    submitted: usize,
}

impl Events {
    fn join(&mut self, slot: PlayerSlot, edict_serial: i32) -> Option<PlayerId> {
        self.players.remove(&slot);
        let serial = NonZeroU64::new(self.serial.checked_add(1)?)?;
        self.serial = serial.get();
        let player = PlayerId::new(slot, serial);
        self.players.insert(slot, (player, edict_serial));
        Some(player)
    }
}

thread_local! {
    static STATE: Cell<State> = const { Cell::new(State::Unqueried) };
    static ENGINE: Cell<Engine> = Cell::new(Engine::default());
    // Only GameShutdown may extract and drop an attached instance. TLS
    // teardown can occur after the engine has shut down.
    static INSTANCE: RefCell<Option<ManuallyDrop<Box<dyn Any>>>> = const { RefCell::new(None) };
    static EVENTS: RefCell<Events> = RefCell::new(Events {
        players: HashMap::new(), serial: 0, pending: VecDeque::new(), submitted: 0,
    });
    static RUNNING: Cell<bool> = const { Cell::new(false) };
    static INDEX_LOOKUP: Cell<Option<bool>> = const { Cell::new(None) };
    // Count paired activation hooks so an inner post hook cannot expose
    // an arena while the outer GameDLL activation is still running.
    static ACTIVATION: Cell<(usize, bool)> = const { Cell::new((0, false)) };
    static SHUTDOWN: Cell<bool> = const { Cell::new(false) };
}

fn wrong_thread() -> bool {
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

pub(crate) fn player_at(slot: PlayerSlot) -> Option<PlayerId> {
    player_session(slot).map(|(id, _)| id)
}

pub(crate) fn player_session(slot: PlayerSlot) -> Option<(PlayerId, i32)> {
    if !engine_available() {
        return None;
    }
    EVENTS.with(|events| events.borrow().players.get(&slot).copied())
}

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

fn contained<T>(callback: impl FnOnce() -> T) -> Option<T> {
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
            // SAFETY: This is the loader's writable output slot.
            unsafe { out.write(id) };
            state.set(State::Queried(Services { id, log, console }));
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

fn make_context<P: Plugin>(scope: &(), services: Services) -> Context<'_> {
    Context::new(
        scope,
        services.id,
        services.log,
        services.console,
        ENGINE.get().print,
        server_command::<P>,
    )
}

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

unsafe extern "C" fn entity_api<P: Plugin>(table: *mut DllFunctions, version: *mut c_int) -> c_int {
    // SAFETY: Metamod supplies the API negotiation arguments.
    unsafe {
        write_hooks(
            table,
            version,
            ENTITY_INTERFACE,
            DllFunctions {
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

unsafe extern "C" fn entity_api_post<P: Plugin>(
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

unsafe extern "C" fn new_api<P: Plugin>(
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

struct ResultWriter(*mut MetaGlobals, MetaResult);

impl Drop for ResultWriter {
    fn drop(&mut self) {
        // SAFETY: The attached server thread owns the hook result, and
        // Metamod keeps the globals live for this entire invocation.
        unsafe { ptr::addr_of_mut!((*self.0).mres).write(self.1) };
    }
}

fn hook(callback: impl FnOnce()) {
    let _ = contained(|| {
        if wrong_thread() {
            return;
        }
        if let State::Attached { globals, .. } = STATE.get() {
            let _result = ResultWriter(globals, MRES_IGNORED);
            engine::invalidate_string_read();
            callback();
        }
    });
}

fn submit<P: Plugin>(event: Event) {
    let State::Attached {
        disabled: false, ..
    } = STATE.get()
    else {
        return;
    };
    let nested = RUNNING.get();
    if SHUTDOWN.get() || matches!(event, Event::Frame) && nested {
        return;
    }
    let accepted = EVENTS.with(|events| {
        let mut events = events.borrow_mut();
        if !nested {
            events.submitted = 0;
        }
        if events.submitted == MAX_EVENTS_PER_DISPATCH {
            return false;
        }
        events.submitted += 1;
        events.pending.push_back(event);
        true
    });
    if accepted {
        drain::<P>();
    } else {
        disable_callbacks();
        // Metamod and engine logging can dispatch hooks. Use stderr
        // directly, after disabling callbacks and releasing the queue borrow.
        let _ = writeln!(
            io::stderr(),
            "[metamod-sdk-rs] warning: plugin {:?} exceeded its dispatch budget of {MAX_EVENTS_PER_DISPATCH} events; user callbacks disabled",
            P::METADATA.name
        );
    }
}

fn disable_callbacks() {
    if let State::Attached {
        services, globals, ..
    } = STATE.get()
    {
        STATE.set(State::Attached {
            services,
            globals,
            disabled: true,
        });
    }
    EVENTS.with(|events| events.borrow_mut().pending.clear());
}

struct Running;

impl Drop for Running {
    fn drop(&mut self) {
        RUNNING.set(false);
    }
}

fn invoke<P: Plugin, T>(
    services: Services,
    callback: impl FnOnce(&mut P, &mut Context<'_>) -> T,
) -> Option<T> {
    contained(|| {
        INSTANCE.with(|instance| {
            let mut slot = instance.borrow_mut();
            let plugin = slot.as_mut()?.downcast_mut::<P>()?;
            let scope = ();
            let mut context = make_context::<P>(&scope, services);
            Some(callback(plugin, &mut context))
        })
    })
    .flatten()
}

fn synchronous<P: Plugin, T: Default>(
    callback: impl FnOnce(&mut P, &mut Context<'_>, *mut MetaGlobals) -> HookResult<T>,
) -> T {
    contained(|| {
        if wrong_thread() {
            return T::default();
        }
        let State::Attached {
            globals,
            services,
            disabled,
        } = STATE.get()
        else {
            return T::default();
        };
        let mut result = ResultWriter(globals, MRES_IGNORED);
        engine::invalidate_string_read();
        if disabled || SHUTDOWN.get() || RUNNING.replace(true) {
            return T::default();
        }
        let running = Running;
        EVENTS.with(|events| events.borrow_mut().submitted = 1);
        let decision = invoke::<P, _>(services, |plugin, context| {
            callback(plugin, context, globals)
        });
        if decision.is_none() {
            disable_callbacks();
        }
        drop(running);
        drain::<P>();
        if SHUTDOWN.get() || matches!(STATE.get(), State::Attached { disabled: true, .. }) {
            return T::default();
        }
        let (status, value) = match decision.unwrap_or_default() {
            HookResult::Ignored => (MRES_IGNORED, T::default()),
            HookResult::Handled => (MRES_HANDLED, T::default()),
            HookResult::Override(value) => (MRES_OVERRIDE, value),
            HookResult::Supercede(value) => (MRES_SUPERCEDE, value),
        };
        result.1 = status;
        value
    })
    .unwrap_or_default()
}

unsafe extern "C" fn client_connect<P: Plugin>(
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
        let bytes = reason.as_bytes();
        let len = bytes.len().min(127);
        // SAFETY: The host supplies a writable 128-byte buffer through the
        // entire hook, including native reentry. The source is Rust-owned.
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr().cast(), reject, len);
            reject.add(len).write(0);
        }
        return 0;
    }
    // Metamod ignores this placeholder for MRES_IGNORED.
    1
}

unsafe extern "C" fn lag_pre<P: Plugin>() -> c_int {
    c_int::from(synchronous::<P, _>(|plugin, context, _| {
        plugin.allow_lag_compensation(context)
    }))
}

unsafe extern "C" fn lag_post<P: Plugin>() -> c_int {
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

fn drain<P: Plugin>() {
    if RUNNING.replace(true) {
        return;
    }
    let _running = Running;
    let State::Attached { services, .. } = STATE.get() else {
        return;
    };

    while !SHUTDOWN.get() {
        let Some(event) = EVENTS.with(|events| events.borrow_mut().pending.pop_front()) else {
            break;
        };
        if matches!(event, Event::Frame) {
            crate::fake_client::poll_removals();
            if SHUTDOWN.get() {
                break;
            }
            if matches!(STATE.get(), State::Attached { disabled: true, .. }) {
                break;
            }
        }
        let result = invoke::<P, _>(services, |plugin, context| match event {
            Event::Frame => plugin.start_frame(context),
            Event::Activate => plugin.server_activate(context),
            Event::Deactivate => plugin.server_deactivate(context),
            Event::Join(player) => plugin.player_join(context, player),
            Event::Disconnect(player) => plugin.player_disconnect(context, player),
            Event::Command(command) => plugin.server_command(context, &command),
            Event::KeyValue(event) => plugin.key_value(context, &event),
        });
        if result.is_none() {
            disable_callbacks();
            break;
        }
    }
    if SHUTDOWN.get()
        && let Some(instance) = INSTANCE.with(|instance| instance.take())
    {
        let mut instance = ManuallyDrop::into_inner(instance);
        if matches!(
            STATE.get(),
            State::Attached {
                disabled: false,
                ..
            }
        ) {
            let _ = contained(|| {
                if let Some(plugin) = instance.downcast_mut::<P>() {
                    plugin.shutdown();
                }
            });
        }
        let _ = contained(|| drop(instance));
    }
}

pub(crate) fn track_fake_client(slot: PlayerSlot, serial: i32) -> Option<PlayerId> {
    EVENTS.with(|events| {
        let mut events = events.borrow_mut();
        if events.players.contains_key(&slot) {
            return None;
        }
        events.join(slot, serial)
    })
}

/// # Safety
/// The loader supplies initialized GameDLL function slots valid through shutdown.
pub unsafe fn capture_game_dll(game: *mut GameDllFunctions) {
    // SAFETY: The export receives the native table from Metamod.
    unsafe { crate::fake_client::capture_game_dll(game) };
}

fn reset_players() {
    crate::fake_client::invalidate();
    if INDEX_LOOKUP.get().is_some() {
        INDEX_LOOKUP.set(Some(true));
    }
    EVENTS.with(|events| events.borrow_mut().players.clear());
}

/// # Safety
/// A non-null aligned edict must be valid for the native index lookup.
/// Its identity fields must remain readable until a map transition or shutdown.
pub(crate) unsafe fn player_slot(edict: *mut edict_t) -> Option<(PlayerSlot, i32)> {
    let index = ENGINE.get().index?;
    if SHUTDOWN.get() {
        return None;
    }
    if INDEX_LOOKUP.get().is_some() {
        // A hook reentered slot resolution through an engine wrapper. The
        // outer edict may no longer identify its original session.
        reset_players();
        return None;
    }
    // SAFETY: The hook supplies the edict and its initialized identity fields.
    let serial = unsafe { crate::trace::live_serial(edict) }?;
    INDEX_LOOKUP.set(Some(false));
    // SAFETY: Metamod supplied a live edict for this synchronous lookup.
    let index = unsafe { index(edict) };
    if INDEX_LOOKUP.replace(None) == Some(false)
        && crate::trace::within_client_capacity(index)
        // SAFETY: No observed map transition or shutdown retired the slot.
        && unsafe { crate::trace::live_serial(edict) } == Some(serial)
    {
        PlayerSlot::new(index).map(|slot| (slot, serial))
    } else {
        None
    }
}

unsafe extern "C" fn server_command<P: Plugin>() {
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

unsafe extern "C" fn key_value<P: Plugin>(_: *mut edict_t, data: *mut raw::KeyValueData) {
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

unsafe extern "C" fn start_frame<P: Plugin>() {
    hook(|| submit::<P>(Event::Frame));
}

unsafe extern "C" fn begin_map(_: *mut edict_t, _: c_int, _: c_int) {
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

unsafe extern "C" fn server_activate<P: Plugin>(_: *mut edict_t, _: c_int, _: c_int) {
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

unsafe extern "C" fn server_deactivate<P: Plugin>() {
    hook(|| {
        let (depth, _) = ACTIVATION.get();
        ACTIVATION.set((depth, depth != 0));
        crate::trace::invalidate_map();
        reset_players();
        submit::<P>(Event::Deactivate);
    });
}

unsafe extern "C" fn player_join<P: Plugin>(edict: *mut edict_t) {
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

unsafe extern "C" fn player_disconnect<P: Plugin>(edict: *mut edict_t) {
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

unsafe extern "C" fn game_shutdown<P: Plugin>() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Metadata;

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
        }));
    }

    #[test]
    fn disconnect_lookup_rejects_a_replaced_edict() {
        unsafe extern "C" fn replace(edict: *const edict_t) -> c_int {
            // SAFETY: The test passes a writable, live edict slot.
            unsafe { (*edict.cast_mut()).serialnumber = 2 };
            1
        }

        #[derive(Default)]
        struct TestPlugin;
        impl Plugin for TestPlugin {
            const METADATA: Metadata = METADATA;
        }

        queried();
        ENGINE.set(Engine {
            index: Some(replace),
            ..Default::default()
        });
        let mut globals = MetaGlobals::default();
        let mut functions = MetaFunctions::default();
        let mut edict = edict_t {
            serialnumber: 1,
            ..Default::default()
        };
        let slot = PlayerSlot::new(1).unwrap();
        // SAFETY: The tables and edict stay live for all hook calls.
        unsafe {
            assert_eq!(
                attach::<TestPlugin>(PT_STARTUP, &mut functions, &raw mut globals),
                1
            );
            let player = EVENTS
                .with(|events| events.borrow_mut().join(slot, 2))
                .unwrap();
            player_disconnect::<TestPlugin>(&raw mut edict);
            assert_eq!(player_at(slot), Some(player));
            player_disconnect::<TestPlugin>(&raw mut edict);
            assert_eq!(player_at(slot), None);
            game_shutdown::<TestPlugin>();
        }
    }

    #[test]
    fn rejected_native_cvar_is_attempted_only_once() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        static ATTEMPTS: AtomicUsize = AtomicUsize::new(0);

        unsafe extern "C" fn missing(_: *const c_char) -> *mut raw::cvar_t {
            ptr::null_mut()
        }

        unsafe extern "C" fn reject(_: *mut raw::cvar_t) {
            ATTEMPTS.fetch_add(1, Ordering::Relaxed);
            // SAFETY: This synthetic reentry runs with live hook globals.
            unsafe { start_frame::<TestPlugin>() };
        }

        #[derive(Default)]
        struct TestPlugin;
        impl Plugin for TestPlugin {
            const METADATA: Metadata = METADATA;
            fn start_frame(&mut self, context: &mut Context<'_>) {
                for name in [
                    crate::console_name!(c"test_rejected"),
                    crate::console_name!(c"TEST_REJECTED"),
                    crate::console_name!(c"test_rejected"),
                ] {
                    assert_eq!(
                        context
                            .server()
                            .register_cvar(name, crate::cvar_value!(c"value")),
                        Err(crate::EngineError::RegistrationRejected)
                    );
                }
            }
        }
        queried();
        ENGINE.set(Engine {
            find_cvar: Some(missing),
            register_cvar: Some(reject),
            ..Default::default()
        });
        let mut globals = MetaGlobals::default();
        let mut functions = MetaFunctions::default();
        // SAFETY: All invocations run on the attached thread with live globals.
        unsafe {
            assert_eq!(
                attach::<TestPlugin>(PT_STARTUP, &mut functions, &raw mut globals),
                1
            );
            start_frame::<TestPlugin>();
            assert!(matches!(
                STATE.get(),
                State::Attached {
                    disabled: false,
                    ..
                }
            ));
            game_shutdown::<TestPlugin>();
        }
        assert_eq!(ATTEMPTS.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn cvar_getter_discards_a_result_invalidated_during_native_reentry() {
        unsafe extern "C" fn find(_: *const c_char) -> *mut raw::cvar_t {
            // The presence check treats this address as an opaque value.
            ptr::dangling_mut()
        }

        unsafe extern "C" fn get(_: *const c_char) -> *const c_char {
            let value = std::ffi::CString::new("original value").unwrap();
            let result = value.as_ptr();
            // Model a native post hook that reenters the SDK and replaces the
            // CVar value after the engine has selected its return pointer.
            unsafe { start_frame::<TestPlugin>() };
            drop(value);
            result
        }

        #[derive(Default)]
        struct TestPlugin;
        impl Plugin for TestPlugin {
            const METADATA: Metadata = METADATA;

            fn start_frame(&mut self, context: &mut Context<'_>) {
                assert_eq!(
                    context.server().cvar(crate::console_name!(c"test_reentry")),
                    Err(crate::EngineError::Unavailable)
                );
            }
        }

        queried();
        ENGINE.set(Engine {
            find_cvar: Some(find),
            get_cvar: Some(get),
            ..Default::default()
        });
        let mut globals = MetaGlobals::default();
        let mut functions = MetaFunctions::default();
        // SAFETY: The hook globals remain live through nested dispatch.
        unsafe {
            assert_eq!(
                attach::<TestPlugin>(PT_STARTUP, &mut functions, &raw mut globals),
                1
            );
            start_frame::<TestPlugin>();
            assert!(registration_available());
            game_shutdown::<TestPlugin>();
        }
    }

    #[test]
    fn cvar_lookup_interrupted_by_shutdown_discards_native_result() {
        unsafe extern "C" fn find(_: *const c_char) -> *mut raw::cvar_t {
            // SAFETY: This test attaches on this thread with live globals.
            unsafe { game_shutdown::<TestPlugin>() };
            ptr::null_mut()
        }

        unsafe extern "C" fn get(_: *const c_char) -> *const c_char {
            ptr::null()
        }

        #[derive(Default)]
        struct TestPlugin;
        impl Plugin for TestPlugin {
            const METADATA: Metadata = METADATA;

            fn start_frame(&mut self, context: &mut Context<'_>) {
                assert_eq!(
                    context
                        .server()
                        .cvar(crate::console_name!(c"test_shutdown")),
                    Err(crate::EngineError::Unavailable)
                );
            }
        }

        queried();
        ENGINE.set(Engine {
            find_cvar: Some(find),
            get_cvar: Some(get),
            ..Default::default()
        });
        let mut globals = MetaGlobals::default();
        let mut functions = MetaFunctions::default();
        // SAFETY: The native tables remain live on this thread through shutdown.
        unsafe {
            assert_eq!(
                attach::<TestPlugin>(PT_STARTUP, &mut functions, &raw mut globals),
                1
            );
            start_frame::<TestPlugin>();
        }
        assert!(SHUTDOWN.get());
        assert!(INSTANCE.with(|instance| instance.borrow().is_none()));
        assert!(matches!(
            STATE.get(),
            State::Attached {
                disabled: false,
                ..
            }
        ));
        assert_eq!(globals.mres, MRES_IGNORED);
    }

    #[test]
    fn fake_clients_wait_for_release_and_reject_reentrant_invalidation() {
        use crate::{FakeClient, FakeClientError, FakeClientState};

        thread_local! {
            static EDICT: Cell<*mut edict_t> = const { Cell::new(ptr::null_mut()) };
            static USER: Cell<c_int> = const { Cell::new(1) };
            static MODE: Cell<u8> = const { Cell::new(0) };
            static PHASE: Cell<u8> = const { Cell::new(0) };
            static HANDLE: RefCell<Option<FakeClient>> = const { RefCell::new(None) };
            static FRAME_CALLS: Cell<u8> = const { Cell::new(0) };
            static KICK_MATCHED: Cell<bool> = const { Cell::new(false) };
        }

        unsafe extern "C" fn allocate(_: *const c_char) -> *mut edict_t {
            if MODE.get() == 2 {
                // SAFETY: The mock keeps the native hook globals live.
                unsafe { server_deactivate::<Fixture>() };
                return ptr::dangling_mut();
            }
            EDICT.get()
        }
        unsafe extern "C" fn index(_: *const edict_t) -> c_int {
            1
        }
        unsafe extern "C" fn entity(_: c_int) -> *mut edict_t {
            if MODE.get() == 3 {
                MODE.set(0);
                // SAFETY: This mock keeps its hook globals live during reentry.
                unsafe { start_frame::<Fixture>() };
            }
            EDICT.get()
        }
        unsafe extern "C" fn userid(_: *mut edict_t) -> c_int {
            USER.get()
        }
        unsafe extern "C" fn connect(
            entity: *mut edict_t,
            _: *const c_char,
            _: *const c_char,
            _: *mut c_char,
        ) -> c_int {
            if MODE.get() == 1 {
                // SAFETY: This mock reenters with the same live edict and hook globals.
                unsafe { player_disconnect::<Fixture>(entity) };
            }
            1
        }
        unsafe extern "C" fn put(_: *mut edict_t) {}
        unsafe extern "C" fn queue(command: *const c_char) {
            // SAFETY: The SDK supplies a live, terminated command during this call.
            KICK_MATCHED.set(unsafe { CStr::from_ptr(command) } == c"kick #1\n");
        }

        #[derive(Default)]
        struct Fixture;
        impl Plugin for Fixture {
            const METADATA: Metadata = METADATA;

            fn start_frame(&mut self, ctx: &mut Context<'_>) {
                let server = ctx.server();
                // SAFETY: This controlled fixture provides all allocation, setup and kick callbacks.
                let create = || unsafe {
                    server.create_fake_client(c"probe", |setup| {
                        setup.connect(c"loopback") && setup.put_in_server()
                    })
                };
                match PHASE.get() {
                    0 => {
                        let client = create().unwrap();
                        assert_eq!(ctx.player_at(client.player().slot()), Some(client.player()));
                        server.request_fake_client_removal(&client).unwrap();
                        assert_eq!(client.state(), FakeClientState::RemovalQueued);
                        HANDLE.with(|handle| handle.replace(Some(client)));
                    }
                    1 => assert!(matches!(create(), Err(FakeClientError::SetupIncomplete))),
                    2 => assert!(matches!(create(), Err(FakeClientError::SetupIncomplete))),
                    3 => FRAME_CALLS.update(|calls| calls + 1),
                    _ => unreachable!(),
                }
            }
        }

        queried();
        ENGINE.set(Engine {
            create_fake_client: Some(allocate),
            queue_server_command: Some(queue),
            index: Some(index),
            entity: Some(entity),
            userid: Some(userid),
            ..Default::default()
        });
        let mut edict = edict_t::default();
        EDICT.set(&raw mut edict);
        let mut engine_globals = EngineGlobals {
            maxClients: 2,
            maxEntities: 8,
            ..Default::default()
        };
        let mut globals = MetaGlobals::default();
        let mut functions = MetaFunctions::default();
        let mut game = DllFunctions {
            pfnClientConnect: Some(connect),
            pfnClientPutInServer: Some(put),
            ..Default::default()
        };
        let mut game_tables = GameDllFunctions {
            dllapi_table: &mut game,
            ..Default::default()
        };
        // SAFETY: All tables remain allocated until after shutdown, and the
        // deliberately stale return is guarded by an observed map change.
        unsafe {
            assert_eq!(
                attach::<Fixture>(PT_STARTUP, &mut functions, &raw mut globals),
                1
            );
            capture_game_dll(&mut game_tables);
            crate::trace::set_globals(&raw mut engine_globals);
            crate::trace::activate_map();
            start_frame::<Fixture>();
            assert!(KICK_MATCHED.get());
            let client = HANDLE.with(|handle| handle.take()).unwrap();
            player_disconnect::<Fixture>(EDICT.get());
            crate::fake_client::poll_removals();
            assert_eq!(client.state(), FakeClientState::RemovalQueued);
            assert_eq!(player_at(client.player().slot()), None);
            USER.set(2);
            PHASE.set(3);
            MODE.set(3);
            start_frame::<Fixture>();
            assert_eq!(FRAME_CALLS.get(), 1);
            assert_eq!(client.state(), FakeClientState::Removed);

            PHASE.set(1);
            MODE.set(1);
            start_frame::<Fixture>();
            assert_eq!(player_at(client.player().slot()), None);
            PHASE.set(2);
            MODE.set(2);
            start_frame::<Fixture>();
            assert_eq!(player_at(client.player().slot()), None);
            assert!(
                registration_available(),
                "plugin assertions must not have panicked"
            );
            assert_eq!(client.state(), FakeClientState::Removed);
            crate::trace::activate_map();
            PHASE.set(0);
            MODE.set(0);
            USER.set(1);
            start_frame::<Fixture>();
            let replacement = HANDLE.with(|handle| handle.take()).unwrap();
            assert_eq!(replacement.player().slot(), client.player().slot());
            assert_ne!(replacement.player(), client.player());
            server_deactivate::<Fixture>();
            assert_eq!(replacement.state(), FakeClientState::Invalidated);
            game_shutdown::<Fixture>();
        }
    }

    #[test]
    fn client_admission_is_synchronous_and_bounds_the_rejection_buffer() {
        #[derive(Default)]
        struct Admission;
        impl Plugin for Admission {
            const METADATA: Metadata = METADATA;
            const CLIENT_CONNECT_PRE: bool = true;

            fn client_connect(
                &mut self,
                _: &mut Context<'_>,
                client: &ClientConnection,
            ) -> ClientAdmission {
                assert_eq!(client.address.as_c_str(), c"loopback");
                match client.name.as_bytes() {
                    b"defer" => ClientAdmission::Defer,
                    b"panic" => panic!("admission panic"),
                    b"exact" => {
                        ClientAdmission::Reject(std::ffi::CString::new(vec![b'x'; 127]).unwrap())
                    }
                    b"utf8" => ClientAdmission::Reject(
                        std::ffi::CString::new(format!("{}é", "x".repeat(126))).unwrap(),
                    ),
                    _ => {
                        let mut nested = [42; 128];
                        // SAFETY: The nested call has live inputs and writable output.
                        let result = unsafe {
                            client_connect::<Self>(
                                ptr::null_mut(),
                                c"nested".as_ptr(),
                                c"loopback".as_ptr(),
                                nested.as_mut_ptr(),
                            )
                        };
                        assert_eq!(result, 1);
                        assert_eq!(nested, [42; 128]);
                        ClientAdmission::Reject(std::ffi::CString::new(vec![b'x'; 200]).unwrap())
                    }
                }
            }
        }

        queried();
        let mut globals = MetaGlobals::default();
        let mut functions = MetaFunctions::default();
        let mut hooks = DllFunctions::default();
        let mut version = ENTITY_INTERFACE;
        // SAFETY: The mock keeps the hook globals and all ABI arguments live.
        unsafe {
            assert_eq!(
                attach::<Admission>(PT_STARTUP, &mut functions, &raw mut globals),
                1
            );
            assert_eq!(entity_api::<Admission>(&mut hooks, &mut version), 1);
            let connect = hooks.pfnClientConnect.unwrap();
            for (step, name) in [
                c"defer".as_ptr(),
                ptr::null(),
                c"reject".as_ptr(),
                c"exact".as_ptr(),
                c"utf8".as_ptr(),
                c"panic".as_ptr(),
                c"reject".as_ptr(),
            ]
            .into_iter()
            .enumerate()
            {
                let mut output = [42; 129];
                let result = connect(
                    ptr::null_mut(),
                    name,
                    c"loopback".as_ptr(),
                    output.as_mut_ptr(),
                );
                if (2..=4).contains(&step) {
                    assert_eq!((result, globals.mres), (0, MRES_SUPERCEDE));
                    assert!(output[..126].iter().all(|&byte| byte == b'x' as c_char));
                    assert_eq!(output[126] as u8, if step == 4 { 0xc3 } else { b'x' });
                    assert_eq!(output[127..], [0, 42]);
                } else {
                    assert_eq!((result, globals.mres), (1, MRES_IGNORED));
                    assert_eq!(output, [42; 129]);
                }
            }
            game_shutdown::<Admission>();
        }
    }

    #[test]
    fn client_admission_discards_rejection_after_nested_shutdown() {
        thread_local! {
            static DROPPED: Cell<bool> = const { Cell::new(false) };
        }

        #[derive(Default)]
        struct Admission;
        impl Plugin for Admission {
            const METADATA: Metadata = METADATA;

            fn client_connect(
                &mut self,
                _: &mut Context<'_>,
                _: &ClientConnection,
            ) -> ClientAdmission {
                // SAFETY: This mock keeps the attached hook globals live.
                unsafe { game_shutdown::<Self>() };
                assert!(!DROPPED.get());
                ClientAdmission::Reject(c"too late".to_owned())
            }
        }

        impl Drop for Admission {
            fn drop(&mut self) {
                DROPPED.set(true);
            }
        }

        queried();
        let mut globals = MetaGlobals::default();
        let mut functions = MetaFunctions::default();
        let mut reason = [42; 128];
        // SAFETY: Mock globals, input strings and output remain live throughout.
        unsafe {
            assert_eq!(
                attach::<Admission>(PT_STARTUP, &mut functions, &raw mut globals),
                1
            );
            assert_eq!(
                client_connect::<Admission>(
                    ptr::null_mut(),
                    c"client".as_ptr(),
                    c"loopback".as_ptr(),
                    reason.as_mut_ptr()
                ),
                1
            );
        }
        assert!(DROPPED.get());
        assert_eq!(globals.mres, MRES_IGNORED);
        assert_eq!(reason, [42; 128]);
    }

    #[test]
    fn synchronous_results_survive_reentry_and_panics_fall_back() {
        #[derive(Default)]
        struct TestPlugin;
        impl Plugin for TestPlugin {
            const METADATA: Metadata = METADATA;
        }

        queried();
        let mut globals = MetaGlobals::default();
        let mut functions = MetaFunctions::default();
        // SAFETY: The hook globals remain live throughout this test.
        assert_eq!(
            unsafe { attach::<TestPlugin>(PT_STARTUP, &mut functions, &raw mut globals) },
            1
        );
        for (decision, expected, value) in [
            (HookResult::Ignored, MRES_IGNORED, false),
            (HookResult::Handled, MRES_HANDLED, false),
            (HookResult::Override(true), MRES_OVERRIDE, true),
            (HookResult::Supercede(true), MRES_SUPERCEDE, true),
        ] {
            let returned = synchronous::<TestPlugin, _>(|_, _, _| {
                let skipped = synchronous::<TestPlugin, bool>(|_, _, _| panic!("nested hook ran"));
                assert!(!skipped);
                submit::<TestPlugin>(Event::Deactivate);
                decision
            });
            assert_eq!(globals.mres, expected);
            assert_eq!(returned, value);
        }
        assert!(!synchronous::<TestPlugin, bool>(|_, _, _| panic!(
            "hook panic"
        )));
        assert_eq!(globals.mres, MRES_IGNORED);
        // SAFETY: Shutdown uses the live hook globals on the attached thread.
        unsafe { game_shutdown::<TestPlugin>() };
    }

    #[derive(Default)]
    struct BudgetPlugin {
        calls: usize,
        peak_pending: usize,
    }

    impl Plugin for BudgetPlugin {
        const METADATA: Metadata = METADATA;

        fn start_frame(&mut self, _: &mut Context<'_>) {
            self.calls += 1;
            for _ in 0..MAX_EVENTS_PER_DISPATCH {
                submit::<Self>(Event::Activate);
                let pending = EVENTS.with(|events| events.borrow().pending.len());
                self.peak_pending = self.peak_pending.max(pending);
            }
        }

        fn server_activate(&mut self, _: &mut Context<'_>) {
            self.calls += 1;
            submit::<Self>(Event::Activate);
        }
    }

    fn exercise_dispatch_budget(first: unsafe extern "C" fn(), expected: (usize, usize)) {
        queried();
        let mut globals = MetaGlobals::default();
        let globals_ptr = &raw mut globals;
        let mut functions = MetaFunctions::default();
        // SAFETY: All calls run on the attached thread with live globals.
        unsafe {
            assert_eq!(
                attach::<BudgetPlugin>(PT_STARTUP, &mut functions, globals_ptr),
                1
            );
            first();
        }
        assert_eq!(globals.mres, MRES_IGNORED);
        assert!(EVENTS.with(|events| events.borrow().pending.is_empty()));
        assert!(matches!(
            STATE.get(),
            State::Attached { disabled: true, .. }
        ));
        assert!(!RUNNING.get());

        // SAFETY: Use the same live native storage as the attached SDK.
        unsafe {
            (*globals_ptr).mres = MRES_UNSET;
            start_frame::<BudgetPlugin>();
        }
        assert_eq!(globals.mres, MRES_IGNORED);
        INSTANCE.with(|instance| {
            let slot = instance.borrow();
            let plugin = slot
                .as_ref()
                .unwrap()
                .downcast_ref::<BudgetPlugin>()
                .unwrap();
            assert_eq!((plugin.calls, plugin.peak_pending), expected);
        });
        // SAFETY: Shutdown runs on the attached thread with live globals.
        unsafe { game_shutdown::<BudgetPlugin>() };
        assert!(INSTANCE.with(|instance| instance.borrow().is_none()));
    }

    #[test]
    fn dispatch_budget_bounds_a_burst_of_pending_events() {
        exercise_dispatch_budget(
            start_frame::<BudgetPlugin>,
            (1, MAX_EVENTS_PER_DISPATCH - 1),
        );
    }

    #[test]
    fn dispatch_budget_stops_a_feedback_loop_with_a_small_queue() {
        unsafe extern "C" fn activate() {
            // SAFETY: The helper runs this on the attached thread.
            unsafe {
                begin_map(ptr::null_mut(), 0, 0);
                server_activate::<BudgetPlugin>(ptr::null_mut(), 0, 0);
            }
        }
        exercise_dispatch_budget(activate, (MAX_EVENTS_PER_DISPATCH, 0));
    }

    #[test]
    fn public_player_slot_constructor_checks_the_engine_index_range() {
        assert_eq!(PlayerSlot::new(0), None);
        const SLOT: PlayerSlot = PlayerSlot::new(7).unwrap();
        const INDEX: i32 = SLOT.get();
        assert_eq!(INDEX, 7);
        assert_eq!(SLOT.to_string(), "7");
        assert_eq!(PlayerSlot::new(INDEX), Some(SLOT));
        assert_eq!(const { PlayerSlot::new(i32::MAX).unwrap() }.get(), i32::MAX);
        assert_eq!(PlayerSlot::new(-1), None);
        assert_eq!(PlayerSlot::new(i32::MIN), None);
    }

    #[test]
    fn player_slot_traits_round_trip_and_reject_invalid_indices() {
        for index in [1, 7, i32::MAX] {
            let slot = PlayerSlot::try_from(index).unwrap();
            assert_eq!(Some(slot), PlayerSlot::new(index));
            assert_eq!(i32::from(slot), index);
        }
        for index in [0, -1, i32::MIN] {
            assert!(PlayerSlot::try_from(index).is_err());
        }
    }

    #[test]
    fn nested_lifecycle_is_deferred_and_panic_disables_user_hooks() {
        #[derive(Default)]
        struct TestPlugin {
            trace: Vec<&'static str>,
        }

        impl Plugin for TestPlugin {
            const METADATA: Metadata = METADATA;

            fn start_frame(&mut self, ctx: &mut Context<'_>) {
                self.trace.push("frame");
                let slot = const { PlayerSlot::new(7).unwrap() };
                let player = EVENTS
                    .with(|events| events.borrow_mut().join(slot, 0))
                    .unwrap();
                assert!(ctx.is_player_current(player));
                // SAFETY: These synthetic hooks run on the attached thread.
                unsafe {
                    start_frame::<Self>();
                    begin_map(ptr::null_mut(), 0, 0);
                    server_activate::<Self>(ptr::null_mut(), 0, 0);
                    server_deactivate::<Self>();
                }
                assert!(!ctx.is_player_current(player));
                self.trace.push("frame end");
            }

            fn server_deactivate(&mut self, _: &mut Context<'_>) {
                self.trace.push("deactivate");
                panic!("disable all callbacks");
            }

            fn server_activate(&mut self, _: &mut Context<'_>) {
                self.trace.push("activate");
            }
        }

        queried();
        let mut globals = MetaGlobals {
            prev_mres: MRES_HANDLED,
            ..Default::default()
        };
        let globals_ptr = &raw mut globals;
        let mut functions = MetaFunctions::default();
        // SAFETY: The test keeps its foreign output storage live.
        unsafe {
            assert_eq!(
                attach::<TestPlugin>(PT_STARTUP, &mut functions, globals_ptr),
                1
            );
            thread::spawn(|| start_frame::<TestPlugin>())
                .join()
                .unwrap();
            assert_eq!(globals.mres, MRES_UNSET);
            start_frame::<TestPlugin>();
            assert_eq!(globals.mres, MRES_IGNORED);
            assert_eq!(globals.prev_mres, MRES_HANDLED);
            // Use the same native pointer for writes between invocations.
            (*globals_ptr).mres = MRES_UNSET;
            begin_map(ptr::null_mut(), 0, 0);
            server_activate::<TestPlugin>(ptr::null_mut(), 0, 0);
        }
        assert_eq!(globals.mres, MRES_IGNORED);
        INSTANCE.with(|instance| {
            let slot = instance.borrow();
            let plugin = slot.as_ref().unwrap().downcast_ref::<TestPlugin>().unwrap();
            assert_eq!(
                plugin.trace,
                ["frame", "frame end", "activate", "deactivate"]
            );
        });
        assert!(matches!(
            STATE.get(),
            State::Attached { disabled: true, .. }
        ));
        // SAFETY: Shutdown is delivered on the same thread with live globals.
        unsafe { game_shutdown::<TestPlugin>() };
        assert!(INSTANCE.with(|instance| instance.borrow().is_none()));
    }

    #[test]
    fn session_ids_expire_on_disconnect_slot_reuse_and_map_boundaries() {
        let slot = const { PlayerSlot::new(7).unwrap() };
        let first = EVENTS
            .with(|events| events.borrow_mut().join(slot, 0))
            .unwrap();
        assert_eq!(player_at(slot), Some(first));
        let ended = EVENTS.with(|events| events.borrow_mut().players.remove(&slot));
        assert_eq!(ended, Some((first, 0)));
        assert_eq!(player_at(slot), None);
        let second = EVENTS
            .with(|events| events.borrow_mut().join(slot, 0))
            .unwrap();
        assert_ne!(first, second);
        reset_players();
        assert_eq!(player_at(slot), None);
        let third = EVENTS
            .with(|events| events.borrow_mut().join(slot, 0))
            .unwrap();
        assert_ne!(second, third);
        let replacement = EVENTS
            .with(|events| events.borrow_mut().join(slot, 0))
            .unwrap();
        assert_ne!(third, replacement);
        assert_eq!(player_at(slot), Some(replacement));
    }

    #[test]
    fn native_print_reentry_keeps_the_plugin_and_borrowed_buffer_live() {
        thread_local! {
            static DROPS: Cell<usize> = const { Cell::new(0) };
        }

        unsafe extern "C" fn print(message: *const c_char) {
            // SAFETY: The SDK lends the mock a C string for this entire call.
            let before = unsafe { engine::copy_string(message, 16) };
            // SAFETY: The test owns the hook globals until both callbacks return.
            unsafe { game_shutdown::<TestPlugin>() };
            let after = unsafe { engine::copy_string(message, 16) };
            // Record the observation without panicking inside the C ABI.
            OBSERVED.set(before.as_deref() == Some(c"owned by plugin") && before == after);
        }

        thread_local! {
            static OBSERVED: Cell<bool> = const { Cell::new(false) };
        }

        struct TestPlugin(std::ffi::CString);

        impl Default for TestPlugin {
            fn default() -> Self {
                Self(c"owned by plugin".to_owned())
            }
        }

        impl Plugin for TestPlugin {
            const METADATA: Metadata = METADATA;

            fn start_frame(&mut self, context: &mut Context<'_>) {
                context.server().print(&self.0);
                assert_eq!(DROPS.get(), 0);
                assert!(!engine_available());
                self.0 = c"frame finished".to_owned();
            }
        }

        impl Drop for TestPlugin {
            fn drop(&mut self) {
                DROPS.update(|count| count + 1);
            }
        }

        queried();
        ENGINE.set(Engine {
            print: Some(print),
            ..Default::default()
        });
        let mut globals = MetaGlobals::default();
        let mut functions = MetaFunctions::default();
        // SAFETY: Both native tables stay live through all mock calls.
        unsafe {
            assert_eq!(
                attach::<TestPlugin>(PT_STARTUP, &mut functions, &raw mut globals),
                1
            );
            start_frame::<TestPlugin>();
            game_shutdown::<TestPlugin>();
        }
        assert!(OBSERVED.get());
        assert_eq!(DROPS.get(), 1);
        assert!(INSTANCE.with(|instance| instance.borrow().is_none()));
        assert_eq!(globals.mres, MRES_IGNORED);
    }

    #[test]
    fn nested_shutdown_revokes_engine_access_and_cleans_up_once() {
        #[derive(Default)]
        struct TestPlugin;

        thread_local! {
            static TRACE: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
        }

        impl Plugin for TestPlugin {
            const METADATA: Metadata = METADATA;

            fn start_frame(&mut self, _: &mut Context<'_>) {
                assert!(engine_available());
                // SAFETY: The test supplies live globals on this thread.
                unsafe { game_shutdown::<Self>() };
                assert!(!engine_available());
                TRACE.with(|trace| trace.borrow_mut().push("frame end"));
            }

            fn shutdown(&mut self) {
                TRACE.with(|trace| trace.borrow_mut().push("shutdown"));
                panic!("cleanup panic");
            }
        }

        impl Drop for TestPlugin {
            fn drop(&mut self) {
                TRACE.with(|trace| trace.borrow_mut().push("drop"));
                panic!("destructor panic");
            }
        }

        queried();
        let mut globals = MetaGlobals::default();
        let globals_ptr = &raw mut globals;
        let mut functions = MetaFunctions::default();
        // SAFETY: All calls use the attached thread and live globals.
        unsafe {
            assert_eq!(
                attach::<TestPlugin>(PT_STARTUP, &mut functions, globals_ptr),
                1
            );
            start_frame::<TestPlugin>();
            assert_eq!(globals.mres, MRES_IGNORED);
            // Use the same native pointer for writes between invocations.
            (*globals_ptr).mres = MRES_UNSET;
            game_shutdown::<TestPlugin>();
            assert_eq!(globals.mres, MRES_IGNORED);
        }
        TRACE.with(|trace| assert_eq!(*trace.borrow(), ["frame end", "shutdown", "drop"]));
        assert!(INSTANCE.with(|instance| instance.borrow().is_none()));
    }

    #[test]
    fn failed_attach_drops_instance_and_allows_retry() {
        #[derive(Default)]
        struct TestPlugin;

        thread_local! {
            static DROPS: Cell<usize> = const { Cell::new(0) };
            static PAYLOAD_DROPS: Cell<usize> = const { Cell::new(0) };
        }

        struct Payload;
        impl Drop for Payload {
            fn drop(&mut self) {
                PAYLOAD_DROPS.set(PAYLOAD_DROPS.get() + 1);
                panic!("panic payload destructor failed");
            }
        }

        impl Plugin for TestPlugin {
            const METADATA: Metadata = METADATA;

            fn initialize(&mut self, _: &mut Context<'_>) -> bool {
                if DROPS.get() == 0 {
                    std::panic::panic_any(Payload);
                }
                false
            }
        }

        impl Drop for TestPlugin {
            fn drop(&mut self) {
                DROPS.set(DROPS.get() + 1);
                panic!("destructor panic");
            }
        }

        queried();
        let mut globals = MetaGlobals::default();
        let mut functions = MetaFunctions::default();
        for drops in 1..=2 {
            // SAFETY: The output pointers remain live during the call.
            assert_eq!(
                unsafe { attach::<TestPlugin>(PT_STARTUP, &mut functions, &raw mut globals) },
                0
            );
            assert_eq!(DROPS.get(), drops);
            assert_eq!(PAYLOAD_DROPS.get(), 1);
            assert!(matches!(STATE.get(), State::Queried(_)));
            assert!(functions.pfnGetEntityAPI2.is_none());
        }
        STATE.set(State::Unqueried);
    }
}
