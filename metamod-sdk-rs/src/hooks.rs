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
///
/// Pre- and post-hooks are enabled independently. Preserve the absence of an
/// original result when another plugin superceded the GameDLL call.
///
/// ```
/// use metamod_sdk_rs::prelude::*;
///
/// #[derive(Default)]
/// struct ObserveLag;
/// impl Plugin for ObserveLag {
/// #    const METADATA: Metadata = Metadata {
/// #        name: c"Observe Lag", version: c"0.1", date: c"",
/// #        author: c"Example", url: c"", log_tag: c"LAG",
/// #    };
///     const LAG_COMPENSATION_POST: bool = true;
///
///     fn allow_lag_compensation_post(&mut self, ctx: &mut Context<'_>, original: Option<bool>)
///         -> PostHookResult<bool>
///     {
///         match original {
///             Some(true) => ctx.server().print(c"GameDLL allowed lag compensation\n"),
///             Some(false) => ctx.server().print(c"GameDLL declined lag compensation\n"),
///             None => {} // No original GameDLL decision to report.
///         }
///         PostHookResult::Ignored
///     }
/// }
/// ```
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
///
/// The hook can run before map activation, so process the owned event without
/// requiring entity discovery or an active map.
///
/// ```
/// use metamod_sdk_rs::prelude::*;
///
/// #[derive(Default)]
/// struct CountSpawns { count: usize }
/// impl Plugin for CountSpawns {
/// #    const METADATA: Metadata = Metadata {
/// #        name: c"Count Spawns", version: c"0.1", date: c"",
/// #        author: c"Example", url: c"", log_tag: c"SPAWNS",
/// #    };
///     const KEYVALUE_PRE: bool = true;
///
///     fn key_value(&mut self, _: &mut Context<'_>, event: &KeyValueEvent) {
///         if event.key.as_c_str() == c"classname" {
///             // Count observed classname assignments, not unique live entities.
///             self.count += 1;
///         }
///     }
/// }
/// ```
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
///
/// Enable the admission pre-hook explicitly. This example rejects one exact
/// name and defers every other connection to the host's normal policy.
///
/// ```
/// use metamod_sdk_rs::prelude::*;
///
/// #[derive(Default)]
/// struct Admission;
/// impl Plugin for Admission {
/// #    const METADATA: Metadata = Metadata {
/// #        name: c"Admission", version: c"0.1", date: c"",
/// #        author: c"Example", url: c"", log_tag: c"ADMISSION",
/// #    };
///     const CLIENT_CONNECT_PRE: bool = true;
///
///     fn client_connect(&mut self, _: &mut Context<'_>, client: &ClientConnection) -> ClientAdmission {
///         if client.name.as_c_str() == c"RejectedExampleName" {
///             ClientAdmission::Reject(c"This name is reserved.".to_owned())
///         } else {
///             ClientAdmission::Defer
///         }
///     }
/// }
/// ```
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum ClientAdmission {
    /// Leave admission to the GameDLL and other Metamod plugins.
    #[default]
    Defer,
    /// Supercede the GameDLL call with a rejection. The reason is truncated
    /// to 127 native bytes and NUL-terminated in the host's output buffer.
    Reject(std::ffi::CString),
}
