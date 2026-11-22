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

/// Owned connection inputs supplied by GoldSrc before a player session exists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClientConnection {
    pub name: std::ffi::CString,
    /// Native address as supplied by the engine, including any port.
    pub address: std::ffi::CString,
}

/// Decision made before the GameDLL handles a client connection.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum ClientAdmission {
    /// Leave admission to the GameDLL and other Metamod plugins.
    #[default]
    Defer,
    /// Supercede the GameDLL call with a rejection. The reason is truncated
    /// to 127 native bytes and NUL-terminated in the host's output buffer.
    Reject(std::ffi::CString),
}
