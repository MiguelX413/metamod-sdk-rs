//! Outcomes for synchronous Metamod hooks.

use crate::bounded_cstr::BoundedCString;
use std::{
    borrow::Borrow,
    ffi::{CStr, CString},
    fmt,
    ops::Deref,
};

/// An owned client rejection reason of at most 127 payload bytes.
///
/// Preserves native bytes, including non-UTF-8 text. Ordinary construction
/// rejects overflow; use [`ClientAdmission::reject_trunc`] to opt into truncation.
/// Successful construction from `&CStr` allocates and copies. Consuming a
/// `CString` or `Box<CStr>` transfers its allocation instead.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ClientRejectReason(BoundedCString<127>);

/// A rejection reason exceeded 127 bytes, excluding its NUL terminator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientRejectReasonTooLong;

impl fmt::Display for ClientRejectReasonTooLong {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("client rejection reasons may contain at most 127 bytes")
    }
}
impl std::error::Error for ClientRejectReasonTooLong {}

impl ClientRejectReason {
    /// Maximum payload length, excluding the terminating NUL.
    pub const MAX_LEN: usize = 127;

    /// Validate and copy native bytes. Returns an error without truncating.
    pub fn new(reason: &CStr) -> Result<Self, ClientRejectReasonTooLong> {
        reason.try_into()
    }

    /// The validated, NUL-terminated reason.
    pub fn as_c_str(&self) -> &CStr {
        self.0.as_c_str()
    }
}

impl TryFrom<&CStr> for ClientRejectReason {
    type Error = ClientRejectReasonTooLong;
    fn try_from(reason: &CStr) -> Result<Self, Self::Error> {
        BoundedCString::try_from(reason)
            .map(Self)
            .map_err(|_| ClientRejectReasonTooLong)
    }
}
impl TryFrom<CString> for ClientRejectReason {
    type Error = ClientRejectReasonTooLong;
    fn try_from(reason: CString) -> Result<Self, Self::Error> {
        BoundedCString::try_from(reason)
            .map(Self)
            .map_err(|_| ClientRejectReasonTooLong)
    }
}
impl TryFrom<Box<CStr>> for ClientRejectReason {
    type Error = ClientRejectReasonTooLong;
    fn try_from(reason: Box<CStr>) -> Result<Self, Self::Error> {
        BoundedCString::try_from(reason)
            .map(Self)
            .map_err(|_| ClientRejectReasonTooLong)
    }
}
impl From<ClientRejectReason> for CString {
    fn from(reason: ClientRejectReason) -> Self {
        reason.0.into()
    }
}
impl From<ClientRejectReason> for Box<CStr> {
    fn from(reason: ClientRejectReason) -> Self {
        reason.0.into()
    }
}
impl Deref for ClientRejectReason {
    type Target = CStr;
    fn deref(&self) -> &CStr {
        self.as_c_str()
    }
}
impl AsRef<CStr> for ClientRejectReason {
    fn as_ref(&self) -> &CStr {
        self.as_c_str()
    }
}
impl Borrow<CStr> for ClientRejectReason {
    fn borrow(&self) -> &CStr {
        self.as_c_str()
    }
}

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
///             ClientAdmission::Reject(c"This name is reserved.".try_into().expect("short reason"))
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
    /// Supercede the GameDLL call with this validated reason. All bytes,
    /// including the terminating NUL, fit the native 128-byte output buffer.
    Reject(ClientRejectReason),
}

impl ClientAdmission {
    /// Validate and copy a rejection reason without truncating.
    ///
    /// ```
    /// use metamod_sdk_rs::{ClientAdmission, ClientRejectReasonTooLong};
    /// let decision = ClientAdmission::reject(c"Server is full")?;
    /// # Ok::<(), ClientRejectReasonTooLong>(())
    /// ```
    pub fn reject(reason: &CStr) -> Result<Self, ClientRejectReasonTooLong> {
        ClientRejectReason::new(reason).map(Self::Reject)
    }

    /// Reject with at most 127 native payload bytes. Truncation can split UTF-8.
    /// Allocates and copies the reason even when it already fits.
    pub fn reject_trunc(reason: &CStr) -> Self {
        Self::Reject(ClientRejectReason(BoundedCString::new_trunc(reason)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejection_reasons_require_explicit_truncation() {
        for len in [0, ClientRejectReason::MAX_LEN] {
            let input = CString::new(vec![0xff; len]).unwrap();
            let copied = ClientRejectReason::new(&input).unwrap();
            assert_eq!(copied.as_c_str(), input.as_c_str());
            assert_ne!(copied.as_ptr(), input.as_ptr());
            let pointer = input.as_ptr();
            let owned = ClientRejectReason::try_from(input).unwrap();
            assert_eq!(owned.as_ptr(), pointer);
            assert_eq!(owned.clone(), owned);
            let boxed: Box<CStr> = owned.into();
            assert_eq!(boxed.as_ptr(), pointer);
            let reason = ClientRejectReason::try_from(boxed).unwrap();
            assert_eq!(reason.as_ptr(), pointer);
            let string: CString = reason.into();
            assert_eq!(string.as_ptr(), pointer);
        }
        let long = CString::new(format!("{}é", "x".repeat(126))).unwrap();
        assert_eq!(
            ClientRejectReason::new(&long).unwrap_err(),
            ClientRejectReasonTooLong
        );
        assert_eq!(
            ClientRejectReason::try_from(long.clone()).unwrap_err(),
            ClientRejectReasonTooLong
        );
        assert_eq!(
            ClientRejectReason::try_from(long.clone().into_boxed_c_str()).unwrap_err(),
            ClientRejectReasonTooLong
        );
        assert_eq!(
            ClientAdmission::reject(&long).unwrap_err(),
            ClientRejectReasonTooLong
        );
        let ClientAdmission::Reject(reason) = ClientAdmission::reject_trunc(&long) else {
            panic!("expected rejection")
        };
        assert_eq!(reason.to_bytes().len(), 127);
        assert_eq!(reason.to_bytes()[126], 0xc3);
        assert_eq!(reason.to_bytes_with_nul()[127], 0);
        assert!(reason.to_str().is_err());
    }
}
