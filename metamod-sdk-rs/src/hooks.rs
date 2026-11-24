//! Outcomes for synchronous Metamod hooks.

/// A pre-hook decision. The value is used only for replacement outcomes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HookResult<T> {
    #[default]
    /// Leave the decision and return value to the GameDLL and other plugins.
    Ignored,
    /// Report that the hook was handled without replacing the return value.
    Handled,
    /// Offer a replacement return value while allowing the GameDLL call.
    /// Metamod combines this decision with those from other plugins.
    Override(T),
    /// Request that Metamod skip the GameDLL call and use this return value.
    Supercede(T),
}

/// A post-hook decision. The original call has already run or been skipped.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PostHookResult<T> {
    #[default]
    /// Leave the result unchanged by this plugin.
    Ignored,
    /// Report handling without supplying a replacement return value.
    Handled,
    /// Offer a replacement for the result after the original dispatch.
    Override(T),
}

/// Owned inputs observed immediately before GameDLL keyvalue dispatch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyValueEvent {
    /// The native classname, if supplied. It may be absent early in loading.
    pub classname: Option<std::ffi::CString>,
    /// Native key bytes copied before GameDLL dispatch.
    pub key: std::ffi::CString,
    /// Native value bytes copied before GameDLL dispatch.
    pub value: std::ffi::CString,
}

/// Owned connection inputs supplied by GoldSrc before a player session exists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClientConnection {
    /// Client-supplied display-name bytes; a tracked player session does not
    /// yet exist at this admission stage.
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
