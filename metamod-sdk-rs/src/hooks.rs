//! Outcomes for synchronous Metamod hooks.

use crate::bounded_cstr::{BoundedCStr, BoundedCString};
use std::{
    borrow::{Borrow, Cow},
    ffi::{CStr, CString},
    fmt,
    ops::Deref,
};

/// A borrowed client rejection reason of at most 127 payload bytes.
///
/// Validation preserves the original bytes and pointer without allocating.
/// Use `Box<ClientRejectReason>` for ownership; consuming conversions from
/// `CString` and `Box<CStr>` reuse their allocations.
#[repr(transparent)]
#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ClientRejectReason(BoundedCStr<127>);

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

    /// Validate native bytes without copying or truncating.
    ///
    /// The result cannot outlive its source:
    ///
    /// ```compile_fail,E0515
    /// use metamod_sdk_rs::ClientRejectReason;
    /// use std::ffi::CString;
    /// fn invalid() -> &'static ClientRejectReason {
    ///     let text = CString::new("temporary").unwrap();
    ///     ClientRejectReason::new(&text).unwrap()
    /// }
    /// ```
    pub const fn new(reason: &CStr) -> Result<&Self, ClientRejectReasonTooLong> {
        let bounded = match BoundedCStr::<127>::new(reason) {
            Ok(value) => value,
            Err(_) => return Err(ClientRejectReasonTooLong),
        };
        // SAFETY: The transparent wrapper preserves the validated field's
        // layout, pointer metadata, and original borrow lifetime.
        Ok(unsafe { &*(bounded as *const BoundedCStr<127> as *const Self) })
    }

    /// The validated, NUL-terminated reason.
    pub const fn as_c_str(&self) -> &CStr {
        self.0.as_c_str()
    }

    fn from_bounded(reason: Box<BoundedCStr<127>>) -> Box<Self> {
        // SAFETY: The transparent wrapper has its validated field's layout
        // and pointer metadata. The allocation is transferred exactly once.
        unsafe { Box::from_raw(Box::into_raw(reason) as *mut Self) }
    }
}
impl<'a> TryFrom<&'a CStr> for &'a ClientRejectReason {
    type Error = ClientRejectReasonTooLong;
    fn try_from(reason: &'a CStr) -> Result<Self, Self::Error> {
        ClientRejectReason::new(reason)
    }
}
impl ToOwned for ClientRejectReason {
    type Owned = Box<Self>;
    fn to_owned(&self) -> Box<Self> {
        Self::from_bounded(self.0.to_owned().into())
    }
}
impl Clone for Box<ClientRejectReason> {
    fn clone(&self) -> Self {
        (**self).to_owned()
    }
}
impl From<&ClientRejectReason> for Box<ClientRejectReason> {
    fn from(reason: &ClientRejectReason) -> Self {
        reason.to_owned()
    }
}
impl TryFrom<&CStr> for Box<ClientRejectReason> {
    type Error = ClientRejectReasonTooLong;
    fn try_from(reason: &CStr) -> Result<Self, Self::Error> {
        ClientRejectReason::new(reason).map(ToOwned::to_owned)
    }
}
impl TryFrom<CString> for Box<ClientRejectReason> {
    type Error = ClientRejectReasonTooLong;
    fn try_from(reason: CString) -> Result<Self, Self::Error> {
        reason.into_boxed_c_str().try_into()
    }
}
impl TryFrom<Box<CStr>> for Box<ClientRejectReason> {
    type Error = ClientRejectReasonTooLong;
    fn try_from(reason: Box<CStr>) -> Result<Self, Self::Error> {
        Box::try_from(reason)
            .map(ClientRejectReason::from_bounded)
            .map_err(|_| ClientRejectReasonTooLong)
    }
}
impl From<Box<ClientRejectReason>> for Box<CStr> {
    fn from(reason: Box<ClientRejectReason>) -> Self {
        // SAFETY: The transparent wrapper has its field's layout and metadata;
        // consuming the box leaves no live borrow of the transferred storage.
        let bounded = unsafe { Box::from_raw(Box::into_raw(reason) as *mut BoundedCStr<127>) };
        bounded.into()
    }
}
impl From<Box<ClientRejectReason>> for CString {
    fn from(reason: Box<ClientRejectReason>) -> Self {
        Box::<CStr>::from(reason).into_c_string()
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
/// Reasons may borrow plugin state or the connection snapshot for the callback's
/// lifetime. The SDK copies their bytes before releasing that borrow and before
/// draining deferred callbacks; a borrowed literal needs no heap allocation.
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
///     fn client_connect<'a>(&'a mut self, _: &mut Context<'_>, client: &'a ClientConnection) -> ClientAdmission<'a> {
///         if client.name.as_c_str() == c"RejectedExampleName" {
///             ClientAdmission::reject(c"This name is reserved.").expect("short reason")
///         } else {
///             ClientAdmission::Defer
///         }
///     }
/// }
/// ```
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum ClientAdmission<'a> {
    /// Leave admission to the GameDLL and other Metamod plugins.
    #[default]
    Defer,
    /// Supercede the GameDLL call with this validated reason. All bytes,
    /// including the terminating NUL, fit the native 128-byte output buffer.
    /// Use [`Self::reject`] to borrow or [`Self::reject_owned`] to consume a CString.
    Reject(Cow<'a, ClientRejectReason>),
}

impl<'a> ClientAdmission<'a> {
    /// Validate and borrow a rejection reason without copying or allocating.
    ///
    /// ```
    /// use metamod_sdk_rs::{ClientAdmission, ClientRejectReasonTooLong};
    /// let decision = ClientAdmission::reject(c"Server is full")?;
    /// # Ok::<(), ClientRejectReasonTooLong>(())
    /// ```
    pub fn reject(reason: &'a CStr) -> Result<Self, ClientRejectReasonTooLong> {
        ClientRejectReason::new(reason).map(|reason| Self::Reject(Cow::Borrowed(reason)))
    }

    /// Validate and take ownership of a dynamic reason, reusing its allocation.
    pub fn reject_owned(reason: CString) -> Result<Self, ClientRejectReasonTooLong> {
        Box::try_from(reason).map(|reason| Self::Reject(Cow::Owned(reason)))
    }

    /// Reject with at most 127 native payload bytes. Truncation can split UTF-8.
    /// Allocates and copies, so the result does not borrow the input.
    pub fn reject_trunc(reason: &CStr) -> Self {
        let bounded = BoundedCString::<127>::new_trunc(reason);
        Self::Reject(Cow::Owned(ClientRejectReason::from_bounded(bounded.into())))
    }
}

impl<'a> From<&'a ClientRejectReason> for ClientAdmission<'a> {
    fn from(reason: &'a ClientRejectReason) -> Self {
        Self::Reject(Cow::Borrowed(reason))
    }
}
impl From<Box<ClientRejectReason>> for ClientAdmission<'_> {
    fn from(reason: Box<ClientRejectReason>) -> Self {
        Self::Reject(Cow::Owned(reason))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn borrowed_reasons_keep_the_source_pointer() {
        const INPUT: &CStr = c"Reserved name";
        const REASON: &ClientRejectReason = match ClientRejectReason::new(INPUT) {
            Ok(reason) => reason,
            Err(_) => panic!("short reason"),
        };
        assert_eq!(REASON.as_ptr(), INPUT.as_ptr());
        let ClientAdmission::Reject(Cow::Borrowed(reason)) =
            ClientAdmission::reject(INPUT).unwrap()
        else {
            panic!("literal should stay borrowed")
        };
        assert_eq!(reason.as_ptr(), INPUT.as_ptr());
        let input = CString::new(vec![0xff; 127]).unwrap();
        let validated: &ClientRejectReason = input.as_c_str().try_into().unwrap();
        let ClientAdmission::Reject(Cow::Borrowed(reason)) = validated.into() else {
            panic!("validated view should stay borrowed")
        };
        assert_eq!(reason.as_ptr(), input.as_ptr());
        let ClientAdmission::Reject(Cow::Borrowed(reason)) =
            ClientAdmission::reject(&input).unwrap()
        else {
            panic!("dynamic string should stay borrowed")
        };
        assert_eq!(reason.as_ptr(), input.as_ptr());
        let pointer = input.as_ptr();
        let ClientAdmission::Reject(Cow::Owned(reason)) =
            ClientAdmission::reject_owned(input).unwrap()
        else {
            panic!("consumed string should stay owned")
        };
        assert_eq!(reason.as_ptr(), pointer);
        let ClientAdmission::Reject(Cow::Owned(reason)) = ClientAdmission::from(reason) else {
            panic!("consumed box should stay owned")
        };
        assert_eq!(reason.as_ptr(), pointer);
    }

    #[test]
    fn rejection_reasons_require_explicit_truncation() {
        for len in [0, ClientRejectReason::MAX_LEN] {
            let input = CString::new(vec![0xff; len]).unwrap();
            let borrowed = ClientRejectReason::new(&input).unwrap();
            assert_eq!(borrowed.as_c_str(), input.as_c_str());
            assert_eq!(borrowed.as_ptr(), input.as_ptr());
            let copied: Box<ClientRejectReason> = borrowed.into();
            assert_eq!(&*copied, borrowed);
            assert_ne!(copied.as_ptr(), borrowed.as_ptr());
            let pointer = input.as_ptr();
            let owned = Box::<ClientRejectReason>::try_from(input).unwrap();
            assert_eq!(owned.as_ptr(), pointer);
            assert_eq!(owned.clone(), owned);
            let boxed: Box<CStr> = owned.into();
            assert_eq!(boxed.as_ptr(), pointer);
            let reason = Box::<ClientRejectReason>::try_from(boxed).unwrap();
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
            Box::<ClientRejectReason>::try_from(long.clone()).unwrap_err(),
            ClientRejectReasonTooLong
        );
        assert_eq!(
            Box::<ClientRejectReason>::try_from(long.clone().into_boxed_c_str()).unwrap_err(),
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
