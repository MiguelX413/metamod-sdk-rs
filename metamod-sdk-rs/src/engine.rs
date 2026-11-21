use crate::{__ffi, Context, PlayerId, PlayerUserId, Server};
use metamod_sys::{
    EngineFunctions,
    raw::{
        MetamodAddServerCommand, MetamodCVarGetPointer, MetamodCVarGetString, MetamodCVarRegister,
        MetamodCVarSetString, MetamodCmd_Argc, MetamodCmd_Argv, MetamodGetPlayerAuthId,
        MetamodGetPlayerUserId, MetamodIndexOfEdict, MetamodPEntityOfEntIndex, MetamodServerPrint,
        MetamodSzFromIndex, MetamodTraceHull, MetamodTraceLine, cvar_t,
    },
};
use std::{
    borrow::Borrow,
    cell::{Cell, RefCell},
    collections::HashSet,
    ffi::{CStr, CString, c_char},
    fmt,
    ops::Deref,
    ptr,
};

macro_rules! engine_callbacks {
    ($($field:ident: $ty:ident = $native:ident),+ $(,)?) => {
        #[derive(Clone, Copy, Default)]
        pub(crate) struct Engine {
            $(pub(crate) $field: $ty,)+
        }

        impl Engine {
            /// # Safety
            /// The loader's table must have aligned, readable, initialized
            /// slots for each field read here. Non-null callbacks must match
            /// the declared ABI and remain callable on the server thread.
            pub(crate) unsafe fn read(table: *mut EngineFunctions) -> Self {
                // SAFETY: Copy just the supplied slots without borrowing foreign memory.
                unsafe { Self { $($field: ptr::addr_of!((*table).$native).read(),)+ } }
            }
        }
    };
}

engine_callbacks! {
    print: MetamodServerPrint = pfnServerPrint,
    index: MetamodIndexOfEdict = pfnIndexOfEdict,
    register_cvar: MetamodCVarRegister = pfnCVarRegister,
    find_cvar: MetamodCVarGetPointer = pfnCVarGetPointer,
    get_cvar: MetamodCVarGetString = pfnCVarGetString,
    set_cvar: MetamodCVarSetString = pfnCVarSetString,
    register_command: MetamodAddServerCommand = pfnAddServerCommand,
    argc: MetamodCmd_Argc = pfnCmd_Argc,
    argv: MetamodCmd_Argv = pfnCmd_Argv,
    entity: MetamodPEntityOfEntIndex = pfnPEntityOfEntIndex,
    userid: MetamodGetPlayerUserId = pfnGetPlayerUserId,
    auth_id: MetamodGetPlayerAuthId = pfnGetPlayerAuthId,
    trace_line: MetamodTraceLine = pfnTraceLine,
    trace_hull: MetamodTraceHull = pfnTraceHull,
    string_from_index: MetamodSzFromIndex = pfnSzFromIndex,
}

// Native registration is irreversible on the portable ABI. Bound the number
// of reservations as well as the storage per reservation.
const MAX_CONSOLE_REGISTRATIONS: usize = 256;
pub(crate) const MAX_ENGINE_STRING: usize = 4096;

pub(crate) type CommandCallback = unsafe extern "C" fn();

thread_local! {
    static CVARS: RefCell<HashSet<&'static CStr>> = RefCell::new(HashSet::new());
    static COMMANDS: RefCell<HashSet<&'static CStr>> = RefCell::new(HashSet::new());
    static READING_NATIVE_STRING: Cell<Option<bool>> = const { Cell::new(None) };
}

/// A command invocation with owned copies of the engine's argument tokens.
#[derive(Clone, Debug)]
pub struct Command {
    name: OwnedConsoleName,
    args: Vec<CString>,
}

impl Command {
    /// The registered command name in lowercase.
    pub fn name(&self) -> &ConsoleName {
        &self.name
    }

    /// One zero-based argument after the command name.
    pub fn arg(&self, index: usize) -> Option<&CStr> {
        self.args.get(index).map(CString::as_c_str)
    }

    /// Parsed tokens after the command name, preserving non-UTF-8 bytes.
    pub fn args(&self) -> &[CString] {
        &self.args
    }
}

/// Owned engine information for a session validated during the lookup.
#[derive(Clone, Debug)]
pub struct PlayerInfo {
    pub id: PlayerId,
    pub user_id: Option<PlayerUserId>,
    pub auth_id: Option<CString>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EngineError {
    Unavailable,
    RegistrationUnavailable,
    InvalidName,
    InvalidValue,
    AlreadyRegistered,
    UnknownCvar,
    RegistrationRejected,
    RegistrationLimit,
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unavailable => "engine operation unavailable",
            Self::RegistrationUnavailable => "registration requires an attached plugin callback",
            Self::InvalidName => {
                "expected a name of 1–63 ASCII letters, digits, underscores or hyphens"
            }
            Self::InvalidValue => "expected a CVar value of at most 1023 bytes",
            Self::AlreadyRegistered => "the console name is already registered",
            Self::UnknownCvar => "unknown CVar",
            Self::RegistrationRejected => "the engine did not register the CVar",
            Self::RegistrationLimit => "the plugin console registration limit was reached",
        })
    }
}

impl std::error::Error for EngineError {}

/// A validated, lowercase name shared by server commands and CVars.
#[repr(transparent)]
#[derive(Debug, Eq, Hash, PartialEq)]
pub struct ConsoleName(CStr);

impl ConsoleName {
    /// Borrow the normalized native name.
    pub const fn as_c_str(&self) -> &CStr {
        &self.0
    }
}

// Share validation and normalization between runtime and const construction.
const fn valid_name(value: &[u8]) -> bool {
    if value.is_empty() || value.len() > 63 {
        return false;
    }
    let mut i = 0;
    while i < value.len() {
        if !value[i].is_ascii_alphanumeric() && !matches!(value[i], b'_' | b'-') {
            return false;
        }
        i += 1;
    }
    true
}

impl ConsoleName {
    /// Borrow a name that is already lowercase. Uppercase input requires normalization.
    pub const fn new(name: &CStr) -> Result<&Self, EngineError> {
        if !valid_name(name.to_bytes()) {
            return Err(EngineError::InvalidName);
        }
        let bytes = name.to_bytes();
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index].is_ascii_uppercase() {
                return Err(EngineError::InvalidName);
            }
            index += 1;
        }
        Ok(Self::from_normalized(name))
    }

    const fn from_normalized(name: &CStr) -> &Self {
        // SAFETY: The transparent wrapper has the same layout and metadata
        // as CStr. Callers have validated the name and its lowercase form.
        unsafe { &*(name as *const CStr as *const Self) }
    }

    #[doc(hidden)]
    pub const fn __normalize<const N: usize>(value: &CStr) -> [u8; N] {
        let bytes = value.to_bytes();
        assert!(valid_name(bytes), "invalid console name");
        assert!(N == bytes.len() + 1, "incorrect console name buffer size");
        let mut normalized = [0; N];
        let mut i = 0;
        while i < bytes.len() {
            normalized[i] = bytes[i].to_ascii_lowercase();
            i += 1;
        }
        normalized
    }

    #[doc(hidden)]
    pub const fn __from_static_bytes(bytes: &'static [u8]) -> &'static Self {
        let Ok(name) = CStr::from_bytes_with_nul(bytes) else {
            panic!("invalid console name terminator");
        };
        match Self::new(name) {
            Ok(name) => name,
            Err(_) => panic!("invalid normalized console name"),
        }
    }
}

impl<'a> TryFrom<&'a CStr> for &'a ConsoleName {
    type Error = EngineError;

    fn try_from(name: &'a CStr) -> Result<Self, Self::Error> {
        ConsoleName::new(name)
    }
}

impl AsRef<CStr> for ConsoleName {
    fn as_ref(&self) -> &CStr {
        self.as_c_str()
    }
}

impl ToOwned for ConsoleName {
    type Owned = OwnedConsoleName;

    fn to_owned(&self) -> Self::Owned {
        OwnedConsoleName(self.as_c_str().to_owned())
    }
}

/// An owned, lowercase console name. Use [Self::normalize] to lowercase input.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct OwnedConsoleName(CString);

impl TryFrom<&CStr> for OwnedConsoleName {
    type Error = EngineError;

    fn try_from(name: &CStr) -> Result<Self, Self::Error> {
        ConsoleName::new(name).map(ToOwned::to_owned)
    }
}

impl TryFrom<CString> for OwnedConsoleName {
    type Error = EngineError;

    fn try_from(name: CString) -> Result<Self, Self::Error> {
        ConsoleName::new(&name)?;
        Ok(Self(name))
    }
}

impl OwnedConsoleName {
    /// Validate an owned C string and lowercase its ASCII letters in place.
    pub fn normalize(name: CString) -> Result<Self, EngineError> {
        if !valid_name(name.as_bytes()) {
            return Err(EngineError::InvalidName);
        }
        if name.as_bytes().iter().any(u8::is_ascii_uppercase) {
            let mut bytes = name.into_bytes_with_nul();
            bytes.make_ascii_lowercase();
            let normalized =
                CString::from_vec_with_nul(bytes).map_err(|_| EngineError::InvalidName)?;
            return Ok(Self(normalized));
        }
        Ok(Self(name))
    }
}

impl Deref for OwnedConsoleName {
    type Target = ConsoleName;

    fn deref(&self) -> &ConsoleName {
        ConsoleName::from_normalized(&self.0)
    }
}

impl Borrow<ConsoleName> for OwnedConsoleName {
    fn borrow(&self) -> &ConsoleName {
        self
    }
}

impl AsRef<ConsoleName> for OwnedConsoleName {
    fn as_ref(&self) -> &ConsoleName {
        self
    }
}

impl From<OwnedConsoleName> for Box<ConsoleName> {
    fn from(name: OwnedConsoleName) -> Self {
        ConsoleName::box_normalized(name.0.into_boxed_c_str())
    }
}

impl ConsoleName {
    /// Validate and lowercase an owned C string in place, reusing its allocation.
    pub fn normalize_boxed(name: Box<CStr>) -> Result<Box<Self>, EngineError> {
        OwnedConsoleName::normalize(name.into_c_string()).map(Box::from)
    }

    fn box_normalized(name: Box<CStr>) -> Box<Self> {
        let raw = Box::into_raw(name);
        // SAFETY: The name is normalized and validated. The transparent
        // wrapper preserves the allocation layout and pointer metadata.
        unsafe { Box::from_raw(raw as *mut Self) }
    }
}

impl TryFrom<Box<CStr>> for Box<ConsoleName> {
    type Error = EngineError;

    /// Validate an already lowercase name and reuse its allocation.
    fn try_from(name: Box<CStr>) -> Result<Self, Self::Error> {
        ConsoleName::new(&name)?;
        Ok(ConsoleName::box_normalized(name))
    }
}

impl From<&ConsoleName> for Box<ConsoleName> {
    fn from(name: &ConsoleName) -> Self {
        name.to_owned().into()
    }
}

impl Clone for Box<ConsoleName> {
    fn clone(&self) -> Self {
        Self::from(self.as_ref())
    }
}

impl From<Box<ConsoleName>> for Box<CStr> {
    /// Recover the underlying C string without copying or reallocating.
    fn from(name: Box<ConsoleName>) -> Self {
        let raw = Box::into_raw(name);
        // SAFETY: ConsoleName is transparent over CStr, so the cast
        // preserves the allocation layout and pointer metadata.
        unsafe { Box::from_raw(raw as *mut CStr) }
    }
}

impl From<Box<ConsoleName>> for OwnedConsoleName {
    /// Take ownership of the normalized name without copying or reallocating.
    fn from(name: Box<ConsoleName>) -> Self {
        Self(Box::<CStr>::from(name).into_c_string())
    }
}

impl TryFrom<CString> for Box<ConsoleName> {
    type Error = EngineError;

    fn try_from(name: CString) -> Result<Self, Self::Error> {
        OwnedConsoleName::try_from(name).map(Self::from)
    }
}

impl TryFrom<&CStr> for Box<ConsoleName> {
    type Error = EngineError;

    fn try_from(name: &CStr) -> Result<Self, Self::Error> {
        OwnedConsoleName::try_from(name).map(Self::from)
    }
}

/// A validated CVar input of at most 1,023 bytes, excluding its terminator.
#[repr(transparent)]
#[derive(Debug, Eq, PartialEq)]
pub struct CvarValue(CStr);

impl CvarValue {
    /// Check the CVar byte limit without allocating.
    pub const fn new(value: &CStr) -> Result<&Self, EngineError> {
        if value.to_bytes().len() > 1023 {
            return Err(EngineError::InvalidValue);
        }
        // SAFETY: The length is validated, and the transparent wrapper
        // preserves the layout, metadata, and borrow lifetime of the CStr.
        Ok(unsafe { &*(value as *const CStr as *const Self) })
    }

    /// Return the original C string without copying or decoding it.
    pub const fn as_c_str(&self) -> &CStr {
        &self.0
    }
}

impl<'a> TryFrom<&'a CStr> for &'a CvarValue {
    type Error = EngineError;

    fn try_from(value: &'a CStr) -> Result<Self, Self::Error> {
        CvarValue::new(value)
    }
}

impl AsRef<CStr> for CvarValue {
    fn as_ref(&self) -> &CStr {
        self.as_c_str()
    }
}

impl From<&CvarValue> for Box<CvarValue> {
    fn from(value: &CvarValue) -> Self {
        CvarValue::box_validated(value.as_c_str().into())
    }
}

impl Clone for Box<CvarValue> {
    fn clone(&self) -> Self {
        Self::from(self.as_ref())
    }
}

impl From<Box<CvarValue>> for Box<CStr> {
    /// Recover the underlying C string without copying or reallocating.
    fn from(value: Box<CvarValue>) -> Self {
        let raw = Box::into_raw(value);
        // SAFETY: CvarValue is transparent over CStr, so the cast
        // preserves the allocation layout and pointer metadata.
        unsafe { Box::from_raw(raw as *mut CStr) }
    }
}

impl TryFrom<CString> for Box<CvarValue> {
    type Error = EngineError;

    fn try_from(value: CString) -> Result<Self, Self::Error> {
        CvarValue::new(&value)?;
        Ok(CvarValue::box_validated(value.into_boxed_c_str()))
    }
}

impl TryFrom<Box<CStr>> for Box<CvarValue> {
    type Error = EngineError;

    /// Validate the byte limit and reuse the input allocation.
    fn try_from(value: Box<CStr>) -> Result<Self, Self::Error> {
        CvarValue::new(&value)?;
        Ok(CvarValue::box_validated(value))
    }
}

impl TryFrom<&CStr> for Box<CvarValue> {
    type Error = EngineError;

    fn try_from(value: &CStr) -> Result<Self, Self::Error> {
        CvarValue::new(value).map(Self::from)
    }
}

impl CvarValue {
    fn box_validated(value: Box<CStr>) -> Box<Self> {
        let raw = Box::into_raw(value);
        // SAFETY: The callers enforce the value length limit. The transparent
        // wrapper preserves the allocation layout and pointer metadata.
        unsafe { Box::from_raw(raw as *mut Self) }
    }
}

fn engine() -> Result<Engine, EngineError> {
    __ffi::engine().ok_or(EngineError::Unavailable)
}

fn available() -> Result<(), EngineError> {
    if __ffi::engine_available() {
        Ok(())
    } else {
        Err(EngineError::Unavailable)
    }
}

fn exists(engine: Engine, name: &CStr) -> Result<bool, EngineError> {
    let find = engine.find_cvar.ok_or(EngineError::Unavailable)?;
    available()?;
    // SAFETY: The synchronous lookup borrows a valid name. We never dereference the result.
    let found = !unsafe { find(name.as_ptr()) }.is_null();
    available()?;
    Ok(found)
}

fn registration_available() -> Result<(), EngineError> {
    if __ffi::registration_available() {
        Ok(())
    } else {
        Err(EngineError::RegistrationUnavailable)
    }
}

fn reserve_name(name: &ConsoleName, cvar: bool) -> Result<&'static CStr, EngineError> {
    CVARS.with(|cvars| {
        COMMANDS.with(|commands| {
            let mut cvars = cvars.borrow_mut();
            let mut commands = commands.borrow_mut();
            if commands.contains(name.as_c_str()) {
                return Err(EngineError::AlreadyRegistered);
            }
            if cvars.contains(name.as_c_str()) {
                return Err(if cvar {
                    EngineError::RegistrationRejected
                } else {
                    EngineError::AlreadyRegistered
                });
            }
            if cvars.len() + commands.len() >= MAX_CONSOLE_REGISTRATIONS {
                return Err(EngineError::RegistrationLimit);
            }
            let name: &'static CStr = Box::leak(name.as_c_str().to_owned().into_boxed_c_str());
            #[cfg(all(test, metamod_lsan))]
            crate::ignore_intentionally_retained(name);
            if cvar {
                cvars.insert(name);
            } else {
                commands.insert(name);
            }
            Ok(name)
        })
    })
}

impl Server<'_, '_> {
    /// Register a server CVar from an attached callback. Use a plugin-specific name.
    pub fn register_cvar(
        &self,
        name: &ConsoleName,
        default: &CvarValue,
    ) -> Result<(), EngineError> {
        registration_available()?;
        let engine = engine()?;
        let register = engine.register_cvar.ok_or(EngineError::Unavailable)?;
        if COMMANDS.with(|commands| commands.borrow().contains(name.as_c_str()))
            || exists(engine, name.as_c_str())?
        {
            return Err(EngineError::AlreadyRegistered);
        }
        registration_available()?;
        // Classic Metamod clones these inputs. ReHLDS can link the record
        // itself into its CVar list and replace the string field with engine
        // storage. The portable ABI does not tell us which path handled the
        // registration. Never reclaim the inputs or read back the record.
        // Reserve before crossing FFI. Even a failed or interrupted
        // attempt may have handed the name and CVar storage to the host.
        let name = reserve_name(name, true)?;
        let initial = Box::leak(default.as_c_str().to_owned().into_boxed_c_str());
        let cvar = Box::new(cvar_t {
            name: name.as_ptr(),
            string: initial.as_ptr(),
            ..Default::default()
        });
        #[cfg(all(test, metamod_lsan))]
        {
            crate::ignore_intentionally_retained(initial);
            crate::ignore_intentionally_retained(cvar.as_ref());
        }
        let cvar = Box::into_raw(cvar);
        // SAFETY: The structure is writable and all input storage stays resident.
        unsafe { register(cvar) };
        if exists(engine, name)? {
            Ok(())
        } else {
            Err(EngineError::RegistrationRejected)
        }
    }

    /// Copy the current value. An empty value differs from an unregistered CVar.
    pub fn cvar(&self, name: &ConsoleName) -> Result<Option<CString>, EngineError> {
        let engine = engine()?;
        let get = engine.get_cvar.ok_or(EngineError::Unavailable)?;
        if !exists(engine, name.as_c_str())? {
            return Ok(None);
        }
        available()?;
        let reading = StringRead::begin().ok_or(EngineError::Unavailable)?;
        // SAFETY: The name lives through the synchronous getter.
        let result = unsafe { get(name.as_c_str().as_ptr()) };
        available()?;
        if !reading.uninterrupted() {
            return Err(EngineError::Unavailable);
        }
        // SAFETY: The returned C string remains readable until the next
        // native call. Checking availability above only reads TLS.
        Ok(unsafe { copy_string(result, MAX_ENGINE_STRING) })
    }

    /// Set an existing CVar through the engine, which copies the supplied value.
    pub fn set_cvar(&self, name: &ConsoleName, new_value: &CvarValue) -> Result<(), EngineError> {
        let engine = engine()?;
        let set = engine.set_cvar.ok_or(EngineError::Unavailable)?;
        if !exists(engine, name.as_c_str())? {
            return Err(EngineError::UnknownCvar);
        }
        available()?;
        // SAFETY: Both input strings stay live through the synchronous copy.
        unsafe { set(name.as_c_str().as_ptr(), new_value.as_c_str().as_ptr()) };
        Ok(())
    }

    /// Register a command for `Plugin::server_command` from an attached callback.
    /// GoldSrc cannot report native command-name collisions through this API.
    pub fn register_command(&self, name: &ConsoleName) -> Result<(), EngineError> {
        registration_available()?;
        let callback = self.context.command;
        let engine = engine()?;
        let register = engine.register_command.ok_or(EngineError::Unavailable)?;
        if engine.argc.is_none() || engine.argv.is_none() {
            return Err(EngineError::Unavailable);
        }
        if exists(engine, name.as_c_str())? {
            return Err(EngineError::AlreadyRegistered);
        }
        registration_available()?;
        let registered = reserve_name(name, false)?;
        // SAFETY: The name and callback stay resident. No registry borrow
        // crosses the native call, which may invoke a command immediately.
        unsafe { register(registered.as_ptr(), Some(callback)) };
        Ok(())
    }
}

/// # Safety
/// A non-null pointer must identify a stable initialized byte range in one
/// allocation through its first NUL or `limit` bytes, whichever comes first.
/// The host must keep those bytes allocated and unmodified during the scan
/// and copy, including while the temporary Rust slice exists.
/// This cannot check whether foreign memory is mapped or still live.
pub(crate) unsafe fn copy_string(value: *const c_char, limit: usize) -> Option<CString> {
    if value.is_null() {
        return None;
    }
    for offset in 0..limit {
        // SAFETY: The caller guarantees readability up to the terminator
        // or the limit. No subsequent byte is touched once a NUL is seen.
        if unsafe { value.add(offset).read() } == 0 {
            // SAFETY: The scan established this range and terminal NUL.
            let bytes = unsafe { std::slice::from_raw_parts(value.cast::<u8>(), offset + 1) };
            return CStr::from_bytes_with_nul(bytes).ok().map(CStr::to_owned);
        }
    }
    None
}

// Observe callbacks that can run after an engine getter has captured its
// result but before its native wrapper returns it to Rust.
pub(crate) struct StringRead;

impl StringRead {
    pub(crate) fn begin() -> Option<Self> {
        if READING_NATIVE_STRING.get().is_some() {
            invalidate_string_read();
            return None;
        }
        READING_NATIVE_STRING.set(Some(false));
        Some(Self)
    }

    pub(crate) fn uninterrupted(&self) -> bool {
        READING_NATIVE_STRING.get() == Some(false)
    }
}

impl Drop for StringRead {
    fn drop(&mut self) {
        READING_NATIVE_STRING.set(None);
    }
}

pub(crate) fn invalidate_string_read() {
    if READING_NATIVE_STRING.get().is_some() {
        READING_NATIVE_STRING.set(Some(true));
    }
}

/// # Safety
/// Called synchronously by the engine with a live command argument context
/// and on the attached server thread.
pub(crate) unsafe fn read_command() -> Option<Command> {
    let reading = StringRead::begin()?;

    let engine = __ffi::engine()?;
    let argc = engine.argc?;
    let argv = engine.argv?;
    // SAFETY: The engine called the command trampoline with a live command context.
    let count = unsafe { argc() };
    if !(1..=128).contains(&count) {
        return None;
    }
    let mut remaining = 4096usize;
    let mut argument = |index| {
        if !reading.uninterrupted() || !__ffi::registration_available() {
            return None;
        }
        // SAFETY: Each index is within argc. Copy before the next native call.
        let value = unsafe { argv(index) };
        if value.is_null() || !reading.uninterrupted() || !__ffi::registration_available() {
            return None;
        }
        // SAFETY: The engine's token remains live until the next native call.
        let value = unsafe { copy_string(value, remaining) }?;
        remaining -= value.as_bytes_with_nul().len();
        Some(value)
    };
    let invoked = argument(0)?;
    let name = OwnedConsoleName::normalize(invoked).ok()?;
    if !COMMANDS.with(|commands| commands.borrow().contains(name.as_c_str())) {
        return None;
    }
    let args = (1..count).map(argument).collect::<Option<_>>()?;
    Some(Command { name, args })
}

impl Context<'_> {
    /// Read engine information for exactly this tracked session. The result
    /// owns its strings, and no engine entity pointer escapes the lookup.
    pub fn player_info(&self, id: PlayerId) -> Option<PlayerInfo> {
        let engine = __ffi::engine()?;
        let (current, serial) = __ffi::player_session(id.slot())?;
        if current != id {
            return None;
        }
        let handle = crate::trace::tracked_entity(id.slot().get(), serial)?;
        let lookup = || {
            if !self.is_player_current(id) {
                return None;
            }
            let edict = handle.resolve(engine)?;
            self.is_player_current(id).then_some(edict)
        };
        let user_id = if let Some(userid) = engine.userid {
            // SAFETY: The edict was just obtained and its session revalidated.
            let uid = unsafe { userid(lookup()?) };
            PlayerUserId::new(uid)
        } else {
            None
        };
        let auth_id = if let Some(auth_id) = engine.auth_id {
            // SAFETY: Resolve and validate the edict again after any prior native call.
            let edict = lookup()?;
            let reading = StringRead::begin()?;
            let result = unsafe { auth_id(edict) };
            // SAFETY: A reentrant callback can change the occupant, but the
            // edict arena remains readable while the map epoch is unchanged.
            if !reading.uninterrupted()
                || !self.is_player_current(id)
                || !unsafe { handle.still_matches(edict) }
            {
                return None;
            }
            // SAFETY: No native call has run since the getter returned a
            // current-session auth string or null.
            unsafe { copy_string(result, MAX_ENGINE_STRING) }
        } else {
            None
        };
        lookup()?;
        Some(PlayerInfo {
            id,
            user_id,
            auth_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_user_id_accepts_only_positive_engine_values() {
        for invalid in [i32::MIN, -1, 0] {
            assert_eq!(PlayerUserId::new(invalid), None);
            assert!(PlayerUserId::try_from(invalid).is_err());
        }
        const VALID: [(i32, PlayerUserId); 3] = [
            (1, PlayerUserId::new(1).unwrap()),
            (42, PlayerUserId::new(42).unwrap()),
            (i32::MAX, PlayerUserId::new(i32::MAX).unwrap()),
        ];
        for (valid, id) in VALID {
            assert_eq!(id.get(), valid);
            assert_eq!(PlayerUserId::try_from(valid), Ok(id));
            assert_eq!(i32::from(id), valid);
            assert_eq!(id.to_string(), valid.to_string());
        }
    }

    #[test]
    fn bounded_string_reads_stop_at_nul_or_budget() {
        // SAFETY: Each input contains a NUL or has at least the specified
        // number of initialized readable bytes.
        unsafe {
            assert_eq!(copy_string(c"".as_ptr(), 4096).as_deref(), Some(c""));
            assert_eq!(copy_string(c"abc".as_ptr(), 4).as_deref(), Some(c"abc"));
            let unterminated = *b"abc";
            assert!(copy_string(unterminated.as_ptr().cast(), 3).is_none());
            assert!(copy_string(ptr::null(), 3).is_none());
        }
    }

    #[test]
    fn native_inputs_still_enforce_console_rules() {
        assert_eq!(
            OwnedConsoleName::normalize(c"Mixed_Name-7".to_owned())
                .as_ref()
                .map(|name| name.as_c_str())
                .map_err(|error| *error),
            Ok(c"mixed_name-7")
        );
        assert_eq!(
            OwnedConsoleName::try_from(c"bad name"),
            Err(EngineError::InvalidName)
        );
        assert_eq!(
            OwnedConsoleName::try_from(c"bad\xff"),
            Err(EngineError::InvalidName)
        );
        assert_eq!(
            OwnedConsoleName::try_from(c""),
            Err(EngineError::InvalidName)
        );
        let maximum = CString::new(vec![b'A'; 63]).unwrap();
        assert_eq!(
            OwnedConsoleName::normalize(maximum)
                .unwrap()
                .as_c_str()
                .to_bytes(),
            &[b'a'; 63]
        );
        let oversized = CString::new(vec![b'A'; 64]).unwrap();
        assert_eq!(
            OwnedConsoleName::try_from(oversized.as_c_str()),
            Err(EngineError::InvalidName)
        );
    }

    #[test]
    fn borrowed_console_names_keep_storage_and_require_lowercase() {
        let input = CString::new("mixed_name-7").unwrap();
        let borrowed: &ConsoleName = input.as_c_str().try_into().unwrap();
        assert_eq!(borrowed.as_c_str().as_ptr(), input.as_ptr());
        assert_eq!(
            ConsoleName::new(c"Mixed_Name-7"),
            Err(EngineError::InvalidName)
        );
        assert_eq!(
            OwnedConsoleName::try_from(c"Mixed_Name-7"),
            Err(EngineError::InvalidName)
        );
        assert_eq!(
            OwnedConsoleName::try_from(c"Mixed_Name-7".to_owned()),
            Err(EngineError::InvalidName)
        );
        let owned = OwnedConsoleName::try_from(input).unwrap();
        let borrowed = ConsoleName::new(c"mixed_name-7").unwrap();
        assert_eq!(owned.as_ref(), borrowed);
        assert!(HashSet::from([owned]).contains(borrowed));
    }

    #[test]
    fn boxed_console_inputs_validate_and_clone() {
        assert_eq!(
            Box::<ConsoleName>::try_from(c"Mixed-Name"),
            Err(EngineError::InvalidName)
        );
        assert_eq!(
            Box::<ConsoleName>::try_from(c"Mixed-Name".to_owned()),
            Err(EngineError::InvalidName)
        );
        let name = Box::<ConsoleName>::try_from(c"mixed-name".to_owned()).unwrap();
        let cloned = name.clone();
        assert_eq!(cloned, name);
        assert_ne!(cloned.as_c_str().as_ptr(), name.as_c_str().as_ptr());
        assert_eq!(name.as_c_str(), c"mixed-name");
        let original = name.as_c_str().as_ptr();
        let native: Box<CStr> = name.into();
        assert_eq!(native.as_ptr(), original);
        assert_eq!(&*native, c"mixed-name");
        let name = Box::<ConsoleName>::try_from(native).unwrap();
        assert_eq!(name.as_c_str().as_ptr(), original);
        assert_eq!(name.as_c_str(), c"mixed-name");
        let uppercase = Box::<CStr>::from(c"Uppercase");
        let original = uppercase.as_ptr();
        let lower = ConsoleName::normalize_boxed(uppercase).unwrap();
        assert_eq!(lower.as_c_str().as_ptr(), original);
        assert_eq!(lower.as_c_str(), c"uppercase");
        assert_eq!(
            Box::<ConsoleName>::try_from(Box::<CStr>::from(c"Uppercase")),
            Err(EngineError::InvalidName)
        );
        assert_eq!(
            ConsoleName::normalize_boxed(Box::<CStr>::from(c"bad name")),
            Err(EngineError::InvalidName)
        );
        assert_eq!(
            Box::<ConsoleName>::try_from(Box::<CStr>::from(c"bad name")),
            Err(EngineError::InvalidName)
        );
        let original = cloned.as_c_str().as_ptr();
        let owned = OwnedConsoleName::from(cloned);
        assert_eq!(owned.as_c_str().as_ptr(), original);
        assert_eq!(owned.as_c_str(), c"mixed-name");
        assert_eq!(
            Box::<ConsoleName>::try_from(c"bad name"),
            Err(EngineError::InvalidName)
        );

        let value = Box::<CvarValue>::try_from(c"\xff").unwrap();
        let cloned = value.clone();
        assert_eq!(cloned, value);
        assert_ne!(cloned.as_c_str().as_ptr(), value.as_c_str().as_ptr());
        assert_eq!(value.as_c_str(), c"\xff");
        let original = value.as_c_str().as_ptr();
        let native: Box<CStr> = value.into();
        assert_eq!(native.as_ptr(), original);
        assert_eq!(&*native, c"\xff");
        let value = Box::<CvarValue>::try_from(native).unwrap();
        assert_eq!(value.as_c_str().as_ptr(), original);
        assert_eq!(value.as_c_str(), c"\xff");
        let oversized = CString::new(vec![b'x'; 1024]).unwrap();
        assert_eq!(
            Box::<CvarValue>::try_from(Box::<CStr>::from(oversized.as_c_str())),
            Err(EngineError::InvalidValue)
        );
        assert_eq!(
            Box::<CvarValue>::try_from(oversized),
            Err(EngineError::InvalidValue)
        );
    }

    #[test]
    fn command_exposes_its_normalized_console_name() {
        let command = Command {
            name: crate::console_name!(c"RUST_HELLO").to_owned(),
            args: Vec::new(),
        };
        let name: &ConsoleName = command.name();
        assert_eq!(name.as_c_str(), c"rust_hello");
        assert_eq!(command.clone().name(), name);
    }

    #[test]
    fn cvar_values_borrow_native_bytes_and_enforce_the_inclusive_limit() {
        assert_eq!(crate::cvar_value!(c"").as_c_str(), c"");
        let maximum = CString::new(vec![0xff_u8; 1023]).unwrap();
        let value = CvarValue::new(maximum.as_c_str()).unwrap();
        assert_eq!(value.as_c_str().as_ptr(), maximum.as_ptr());
        assert_eq!(value.as_c_str().to_bytes(), &[0xff; 1023]);
        let oversized = CString::new(vec![b'x'; 1024]).unwrap();
        assert_eq!(
            CvarValue::new(oversized.as_c_str()),
            Err(EngineError::InvalidValue)
        );
    }

    #[test]
    fn irreversible_registrations_share_a_bounded_name_budget() {
        assert!(reserve_name(crate::console_name!(c"Test"), true).is_ok());
        assert_eq!(
            reserve_name(crate::console_name!(c"test"), true),
            Err(EngineError::RegistrationRejected)
        );
        assert_eq!(
            reserve_name(crate::console_name!(c"TEST"), false),
            Err(EngineError::AlreadyRegistered)
        );
        for index in 1..MAX_CONSOLE_REGISTRATIONS {
            assert!(
                reserve_name(
                    &OwnedConsoleName::try_from(
                        CString::new(format!("test{index}")).unwrap().as_c_str()
                    )
                    .unwrap(),
                    false
                )
                .is_ok()
            );
        }
        assert_eq!(
            reserve_name(crate::console_name!(c"one_more"), true),
            Err(EngineError::RegistrationLimit)
        );
    }
}
