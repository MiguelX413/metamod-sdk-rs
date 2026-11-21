//! Outcomes for synchronous Metamod hooks.

/// A pre-hook decision. The value is used only for replacement outcomes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HookResult<T> {
    #[default]
    Ignored,
    Handled,
    Override(T),
    Supercede(T),
}

/// A post-hook decision. The original call has already run or been skipped.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PostHookResult<T> {
    #[default]
    Ignored,
    Handled,
    Override(T),
}

/// Owned inputs observed immediately before GameDLL keyvalue dispatch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyValueEvent {
    /// The native classname, if supplied. It may be absent early in loading.
    pub classname: Option<std::ffi::CString>,
    pub key: std::ffi::CString,
    pub value: std::ffi::CString,
}
