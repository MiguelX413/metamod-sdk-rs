//! Loading-phase resource registration and map-scoped indices.
use crate::{__ffi, Context, trace};
use metamod_sys::raw::edict_t;
use std::{cell::Cell, ffi::CStr, fmt, marker::PhantomData, rc::Rc};

/// A relative model or sound path: 1–63 ASCII letters, digits, `_`, `-`,
/// `.`, or `/`, without empty, `.` or `..` components. Case is preserved.
/// Sound paths are relative to `sound/`; model paths are relative to the mod.
/// Inline models, sentence names, absolute paths and backslashes are excluded.
#[repr(transparent)]
#[derive(Debug, Eq, PartialEq)]
pub struct ResourcePath(CStr);

/// A resource path is empty, oversized, or outside the supported syntax.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidResourcePath;
impl fmt::Display for InvalidResourcePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "expected a relative ASCII resource path of 1–63 bytes without dot or empty components",
        )
    }
}
impl std::error::Error for InvalidResourcePath {}

impl ResourcePath {
    /// Validate without allocating. This checks syntax, not file existence or
    /// asset validity. Engine load failures can terminate the map/server.
    ///
    /// ```
    /// use metamod_sdk_rs::ResourcePath;
    /// const MODEL: &ResourcePath = ResourcePath::new(c"models/example.mdl").unwrap();
    /// ```
    /// ```compile_fail
    /// use metamod_sdk_rs::ResourcePath;
    /// const BAD: &ResourcePath = ResourcePath::new(c"../outside.mdl").unwrap();
    /// ```
    pub const fn new(value: &CStr) -> Option<&Self> {
        let bytes = value.to_bytes();
        if bytes.is_empty() || bytes.len() > 63 {
            return None;
        }
        let mut start = 0;
        let mut i = 0;
        while i <= bytes.len() {
            if i == bytes.len() || bytes[i] == b'/' {
                let len = i - start;
                if len == 0
                    || (len == 1 && bytes[start] == b'.')
                    || (len == 2 && bytes[start] == b'.' && bytes[start + 1] == b'.')
                {
                    return None;
                }
                start = i + 1;
            } else if !(bytes[i].is_ascii_alphanumeric()
                || bytes[i] == b'_'
                || bytes[i] == b'-'
                || bytes[i] == b'.')
            {
                return None;
            }
            i += 1;
        }
        // SAFETY: Transparent wrapper over the same CStr allocation and
        // metadata. The shared view preserves the input lifetime and bytes.
        Some(unsafe { &*(value as *const CStr as *const Self) })
    }

    /// Original validated bytes, including their existing NUL terminator.
    pub const fn as_c_str(&self) -> &CStr {
        &self.0
    }
}
impl<'a> TryFrom<&'a CStr> for &'a ResourcePath {
    type Error = InvalidResourcePath;
    fn try_from(value: &'a CStr) -> Result<Self, Self::Error> {
        ResourcePath::new(value).ok_or(InvalidResourcePath)
    }
}

/// Registration was unavailable, interrupted, or returned a nonpositive index.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrecacheError {
    /// The loading scope ended, was interrupted, or is on another thread.
    Unavailable,
    /// The host did not supply the requested callback.
    Unsupported,
    /// The host returned zero or a negative index.
    Rejected,
}
impl fmt::Display for PrecacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unavailable => "the resource loading scope is unavailable",
            Self::Unsupported => "the host resource callback is unavailable",
            Self::Rejected => "the host returned an invalid resource index",
        })
    }
}
impl std::error::Error for PrecacheError {}

macro_rules! index_type {
    ($name:ident, $check:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
        pub struct $name {
            index: i32,
            generation: u64,
        }
        impl $name {
            /// Native index for diagnostics or raw integration. Reuse on a
            /// different map does not preserve this handle's identity.
            pub const fn get(self) -> i32 {
                self.index
            }
        }
        impl Context<'_> {
            /// Check that the index belongs to the current active resource map.
            /// This does not validate a client protocol or pin the map.
            pub fn $check(&self, index: $name) -> bool {
                let state = STATE.get();
                state.map.is_some()
                    && trace::active_map() == state.map
                    && state.generation == Some(index.generation)
            }
        }
    };
}
index_type!(
    ModelIndex,
    is_model_current,
    "A positive model index registered for one map."
);
index_type!(
    SoundIndex,
    is_sound_current,
    "A positive sound index registered for one map."
);

/// Scoped registration during [`crate::Plugin::precache`], before worldspawn.
/// Names must be static because classic engines can retain their pointers.
/// This type cannot be constructed by plugins, sent between threads, or kept
/// beyond the callback. Generic downloads and event scripts remain raw APIs.
pub struct Precache<'loading> {
    generation: u64,
    scope: PhantomData<(&'loading mut (), Rc<()>)>,
}
impl Precache<'_> {
    /// Register a model (including sprites) using a static validated path.
    /// Missing/corrupt assets or native table exhaustion can cause fatal host
    /// errors; these are not converted to `PrecacheError`.
    ///
    /// ```compile_fail
    /// use metamod_sdk_rs::{Precache, ResourcePath};
    /// fn temporary(resources: &mut Precache<'_>) {
    ///     let name = std::ffi::CString::new("models/example.mdl").unwrap();
    ///     resources.model(ResourcePath::new(&name).unwrap());
    /// }
    /// ```
    pub fn model(&mut self, path: &'static ResourcePath) -> Result<ModelIndex, PrecacheError> {
        let engine = self.engine()?;
        let callback = engine.precache_model.ok_or(PrecacheError::Unsupported)?;
        // SAFETY: The worldspawn loading scope is current, and the static,
        // validated name survives retention and synchronous native reentry.
        let index = unsafe { callback(path.as_c_str().as_ptr()) };
        self.engine()?;
        if index <= 0 {
            return Err(PrecacheError::Rejected);
        }
        Ok(ModelIndex {
            index,
            generation: self.generation,
        })
    }

    /// Register a sound path relative to `sound/`. The same native failure
    /// and static-name requirements as [`Self::model`] apply.
    pub fn sound(&mut self, path: &'static ResourcePath) -> Result<SoundIndex, PrecacheError> {
        let engine = self.engine()?;
        let callback = engine.precache_sound.ok_or(PrecacheError::Unsupported)?;
        // SAFETY: Same retained-string and loading-phase contract as model.
        let index = unsafe { callback(path.as_c_str().as_ptr()) };
        self.engine()?;
        if index <= 0 {
            return Err(PrecacheError::Rejected);
        }
        Ok(SoundIndex {
            index,
            generation: self.generation,
        })
    }

    fn engine(&self) -> Result<crate::engine::Engine, PrecacheError> {
        let engine = __ffi::engine().ok_or(PrecacheError::Unavailable)?;
        let state = STATE.get();
        if state.generation != Some(self.generation) || !state.open {
            return Err(PrecacheError::Unavailable);
        }
        Ok(engine)
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct State {
    generation: Option<u64>,
    seen: bool,
    open: bool,
    map: Option<u64>,
}
thread_local! {
    static STATE: Cell<State> = const { Cell::new(State { generation: Some(0), seen: false, open: false, map: None }) };
    static IDENTIFYING: Cell<bool> = const { Cell::new(false) };
}
pub(crate) fn invalidate() {
    let old = STATE.get();
    STATE.set(State {
        generation: old.generation.and_then(|v| v.checked_add(1)),
        seen: false,
        open: false,
        map: None,
    });
}
pub(crate) fn close() {
    STATE.update(|state| State {
        open: false,
        ..state
    });
}

pub(crate) fn begin_activation() {
    if STATE.get().map.is_some() {
        invalidate();
    } else {
        close();
    }
}
pub(crate) fn activated() {
    STATE.update(|state| State {
        map: if state.seen {
            trace::active_map()
        } else {
            None
        },
        ..state
    });
}

pub(crate) fn world_spawn(edict: *mut edict_t, callback: impl FnOnce(&mut Precache<'_>)) {
    let Some(engine) = __ffi::engine() else {
        return;
    };
    // Runtime spawns cannot grant a loading capability, even when a host did
    // not deliver a worldspawn that this plugin could observe at startup.
    if trace::active_map().is_some() {
        return;
    }
    let Some(lookup) = engine.entity else { return };
    if edict.is_null() || IDENTIFYING.replace(true) {
        return;
    }
    struct LookupGuard;
    impl Drop for LookupGuard {
        fn drop(&mut self) {
            IDENTIFYING.set(false);
        }
    }
    let guard = LookupGuard;
    let before = STATE.get();
    // SAFETY: Index zero is the world during native Spawn, including loading.
    // Only compare the returned address; do not dereference either pointer.
    let world = unsafe { lookup(0) };
    drop(guard);
    if world != edict || STATE.get() != before || !__ffi::engine_available() {
        return;
    }
    close();
    if before.seen {
        return;
    }
    let Some(generation) = before.generation.and_then(|v| v.checked_add(1)) else {
        return;
    };
    STATE.set(State {
        generation: Some(generation),
        seen: true,
        open: true,
        map: None,
    });
    struct Window(u64);
    impl Drop for Window {
        fn drop(&mut self) {
            if STATE.get().generation == Some(self.0) {
                close();
            }
        }
    }
    let _window = Window(generation);
    callback(&mut Precache {
        generation,
        scope: PhantomData,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_validate_bytes_without_allocation() {
        const PATH: &ResourcePath = ResourcePath::new(c"models/Test-1.mdl").unwrap();
        assert_eq!(PATH.as_c_str(), c"models/Test-1.mdl");
        for value in [
            c"",
            c"/x",
            c"x/",
            c"x//y",
            c"./x",
            c"x/../y",
            c"x\\y",
            c"!sentence",
            c"*1",
            c"x y",
            c"\xff",
        ] {
            assert!(ResourcePath::new(value).is_none());
        }
        let max = std::ffi::CString::new(vec![b'x'; 63]).unwrap();
        assert_eq!(
            ResourcePath::new(&max).unwrap().as_c_str().as_ptr(),
            max.as_ptr()
        );
        assert!(ResourcePath::new(&std::ffi::CString::new(vec![b'x'; 64]).unwrap()).is_none());
    }
}
