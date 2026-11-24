//! Owned user-message payloads, registration lookup, and recipient validation.

use std::{cell::Cell, ffi::CStr, fmt};

/// Maximum user-message payload size in bytes, excluding protocol framing.
pub const MAX_USER_MESSAGE_BYTES: usize = 192;

/// A user-message payload assembled from GoldSrc wire fields.
///
/// Building a payload makes no engine calls. Before sending, check the client
/// protocol, registered size, and recipient. Native sending requires the unsafe
/// host opt-in through `Context::message_sender`.
///
/// ```
/// use metamod_sdk_rs::UserMessagePayload;
///
/// # fn main() -> Result<(), metamod_sdk_rs::MessagePayloadError> {
/// let mut payload = UserMessagePayload::new();
/// payload.write_byte(7)?;
/// payload.write_short(-2)?;
/// payload.write_string(c"ready")?;
/// assert_eq!(payload.as_bytes(), b"\x07\xfe\xffready\0");
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct UserMessagePayload {
    bytes: [u8; MAX_USER_MESSAGE_BYTES],
    len: usize,
}

/// A payload exceeded its byte budget or did not match its fixed size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessagePayloadError {
    /// Appending a field would exceed [`MAX_USER_MESSAGE_BYTES`].
    /// The failed append leaves the existing payload unchanged.
    TooLong,
    /// The encoded length differs from a fixed-size registration or schema.
    SizeMismatch {
        /// Required payload length in bytes, excluding framing.
        expected: usize,
        /// Encoded payload length in bytes, excluding framing.
        actual: usize,
    },
}

impl fmt::Display for MessagePayloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLong => write!(
                f,
                "user-message payload exceeds {MAX_USER_MESSAGE_BYTES} bytes"
            ),
            Self::SizeMismatch { expected, actual } => {
                write!(f, "expected {expected} payload bytes, found {actual}")
            }
        }
    }
}

impl std::error::Error for MessagePayloadError {}

impl Default for UserMessagePayload {
    fn default() -> Self {
        Self::new()
    }
}

impl UserMessagePayload {
    /// Create an empty payload without accessing the engine.
    pub const fn new() -> Self {
        Self {
            bytes: [0; MAX_USER_MESSAGE_BYTES],
            len: 0,
        }
    }

    /// Encoded payload bytes, without the message ID or size prefix.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }

    /// Check the encoded length against a fixed-size message schema.
    pub fn validate_fixed_size(&self, expected: usize) -> Result<(), MessagePayloadError> {
        if self.len == expected {
            Ok(())
        } else {
            Err(MessagePayloadError::SizeMismatch {
                expected,
                actual: self.len,
            })
        }
    }

    /// Append an unsigned byte.
    pub fn write_byte(&mut self, value: u8) -> Result<(), MessagePayloadError> {
        self.append(&[value])
    }

    /// Append a signed byte.
    pub fn write_char(&mut self, value: i8) -> Result<(), MessagePayloadError> {
        self.append(&value.to_le_bytes())
    }

    /// Append a little-endian signed 16-bit integer.
    pub fn write_short(&mut self, value: i16) -> Result<(), MessagePayloadError> {
        self.append(&value.to_le_bytes())
    }

    /// Append a little-endian signed 32-bit integer.
    pub fn write_long(&mut self, value: i32) -> Result<(), MessagePayloadError> {
        self.append(&value.to_le_bytes())
    }

    /// Copy native string bytes, including the terminal NUL.
    ///
    /// A failed field append leaves earlier fields intact. Include the string's
    /// NUL byte when calculating the payload budget.
    ///
    /// ```
    /// use metamod_sdk_rs::{MAX_USER_MESSAGE_BYTES, MessagePayloadError, UserMessagePayload};
    /// use std::ffi::CString;
    ///
    /// let mut payload = UserMessagePayload::new();
    /// payload.write_byte(7)?;
    /// let oversized = CString::new(vec![b'x'; MAX_USER_MESSAGE_BYTES]).unwrap();
    /// assert_eq!(payload.write_string(&oversized), Err(MessagePayloadError::TooLong));
    /// assert_eq!(payload.as_bytes(), &[7]);
    /// # Ok::<(), MessagePayloadError>(())
    /// ```
    pub fn write_string(&mut self, value: &CStr) -> Result<(), MessagePayloadError> {
        self.append(value.to_bytes_with_nul())
    }

    // A failed field write must leave the existing payload intact.
    fn append(&mut self, field: &[u8]) -> Result<(), MessagePayloadError> {
        if field.len() > MAX_USER_MESSAGE_BYTES - self.len {
            return Err(MessagePayloadError::TooLong);
        }
        let end = self.len + field.len();
        self.bytes[self.len..end].copy_from_slice(field);
        self.len = end;
        Ok(())
    }
}

/// Payload size declared by the native user-message registration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UserMessageSize {
    /// Every payload must contain exactly this many bytes, excluding framing.
    Fixed(u8),
    /// Payload length may vary up to [`MAX_USER_MESSAGE_BYTES`].
    Variable,
}

/// An observation of a registered user message on the active map.
/// Resolve it again after changing maps or before a future native send.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegisteredUserMessage {
    id: u8,
    size: UserMessageSize,
}

impl RegisteredUserMessage {
    /// Native message ID observed at lookup time (64 through 255).
    pub const fn id(self) -> u8 {
        self.id
    }

    /// Fixed or variable payload size observed at lookup time.
    pub const fn size(self) -> UserMessageSize {
        self.size
    }

    /// Check the encoded byte count against this observed registration.
    /// This does not validate field meaning, client readiness, or whether the
    /// registration is still current. Variable-size payloads pass this check.
    pub fn validate_payload(self, payload: &UserMessagePayload) -> Result<(), MessagePayloadError> {
        match self.size {
            UserMessageSize::Fixed(expected) => payload.validate_fixed_size(usize::from(expected)),
            UserMessageSize::Variable => Ok(()),
        }
    }

    fn from_native(id: i32, size: i32) -> Result<Self, MessageLookupError> {
        if id == 0 {
            return Err(MessageLookupError::NotRegistered);
        }
        let id = u8::try_from(id)
            .ok()
            .filter(|id| *id >= 64)
            .ok_or(MessageLookupError::InvalidRegistration)?;
        let size = match size {
            -1 => UserMessageSize::Variable,
            0.. => {
                let size = u8::try_from(size)
                    .ok()
                    .filter(|size| usize::from(*size) <= MAX_USER_MESSAGE_BYTES)
                    .ok_or(MessageLookupError::InvalidRegistration)?;
                UserMessageSize::Fixed(size)
            }
            _ => return Err(MessageLookupError::InvalidRegistration),
        };
        Ok(Self { id, size })
    }
}

/// A user-message registration could not be resolved on the active map.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageLookupError {
    /// The map, Metamod lookup utility, or engine access is unavailable,
    /// including lifecycle invalidation during lookup.
    Unavailable,
    /// Metamod returned zero for this name, indicating no registration.
    NotRegistered,
    /// The native ID is outside 64–255, or the size is neither variable
    /// (`-1`) nor a fixed length of 0–192 bytes.
    InvalidRegistration,
}

impl fmt::Display for MessageLookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unavailable => "user-message lookup requires an active map and Metamod utility",
            Self::NotRegistered => "user message is not registered",
            Self::InvalidRegistration => "host returned an invalid user-message ID or size",
        })
    }
}

impl std::error::Error for MessageLookupError {}

impl crate::MetaMod<'_, '_> {
    /// Look up a registered user message by its native name. A missing
    /// registration is distinct from a missing utility or active map.
    ///
    /// Check a prepared payload against the current registration without sending
    /// anything. The name and field layout must come from the GameDLL/client
    /// protocol; this only checks availability and registered byte count.
    ///
    /// ```
    /// use metamod_sdk_rs::{Context, UserMessagePayload};
    /// use std::ffi::CStr;
    ///
    /// fn validate(ctx: &Context<'_>, name: &CStr, payload: &UserMessagePayload)
    ///     -> Result<(), Box<dyn std::error::Error>>
    /// {
    ///     let registration = ctx.metamod().user_message(name)?;
    ///     registration.validate_payload(payload)?;
    ///     Ok(())
    /// }
    /// ```
    pub fn user_message(&self, name: &CStr) -> Result<RegisteredUserMessage, MessageLookupError> {
        let map = crate::trace::active_map().ok_or(MessageLookupError::Unavailable)?;
        let lookup = self
            .context
            .user_message
            .ok_or(MessageLookupError::Unavailable)?;
        // A positive ID without an initialized size is invalid.
        let mut size = i32::MIN;
        // SAFETY: The loader supplies the utility and plugin ID. The input
        // string and writable size slot remain live through native reentry.
        let id = unsafe { lookup(self.context.id, name.as_ptr(), &mut size) };
        if crate::trace::active_map() != Some(map) {
            return Err(MessageLookupError::Unavailable);
        }
        RegisteredUserMessage::from_native(id, size)
    }
}

/// A player session observed to be current during recipient validation.
/// Later operations must revalidate the session before using it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MessageRecipient(crate::PlayerId);

impl MessageRecipient {
    /// Return the session observed during validation. This does not revalidate it.
    pub const fn player(self) -> crate::PlayerId {
        self.0
    }
}

/// The recipient could not be resolved to a live tracked session on the
/// active map. Engine unavailability also prevents this validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaleMessageRecipient;

impl fmt::Display for StaleMessageRecipient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("message recipient is not a live tracked player on the active map")
    }
}

impl std::error::Error for StaleMessageRecipient {}

impl crate::Context<'_> {
    /// Check the tracked session and its live edict without exposing a pointer.
    /// The caller must still establish client protocol readiness.
    pub fn message_recipient(
        &self,
        player: crate::PlayerId,
    ) -> Result<MessageRecipient, StaleMessageRecipient> {
        self.resolve_message_recipient(player)?;
        Ok(MessageRecipient(player))
    }

    fn resolve_message_recipient(
        &self,
        player: crate::PlayerId,
    ) -> Result<*mut metamod_sys::raw::edict_t, StaleMessageRecipient> {
        let engine = crate::__ffi::engine().ok_or(StaleMessageRecipient)?;
        let (current, serial) =
            crate::__ffi::player_session(player.slot()).ok_or(StaleMessageRecipient)?;
        if current != player {
            return Err(StaleMessageRecipient);
        }
        let entity = crate::trace::tracked_entity(player.slot().get(), serial)
            .ok_or(StaleMessageRecipient)?;
        let edict = entity.resolve(engine).ok_or(StaleMessageRecipient)?;
        // The engine lookup can synchronously disconnect or replace the player.
        if crate::__ffi::player_session(player.slot()) != Some((player, serial)) {
            return Err(StaleMessageRecipient);
        }
        Ok(edict)
    }
}

thread_local! {
    static SENDING: Cell<bool> = const { Cell::new(false) };
}

// Releases only SDK bookkeeping. Native ownership may be lost after interruption.
struct SendGuard;

impl SendGuard {
    fn acquire() -> Result<Self, MessageSendError> {
        if SENDING.replace(true) {
            return Err(MessageSendError::Busy);
        }
        Ok(Self)
    }
}

impl Drop for SendGuard {
    fn drop(&mut self) {
        SENDING.set(false);
    }
}

/// Reliable unicast within a callback, subject to the unsafe host contract.
pub struct MessageSender<'context, 'callback> {
    context: &'context crate::Context<'callback>,
}

/// A scoped send was rejected or interrupted. A failure after native begin
/// does not trigger an automatic native end or roll back emitted bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageSendError {
    /// Another SDK send is active on this thread, including its validation phase.
    /// This guard does not detect arbitrary native senders.
    Busy,
    /// Native messaging callbacks, the active map, or engine access are unavailable.
    Unavailable,
    /// The message registration could not be resolved again at the send boundary.
    Lookup(MessageLookupError),
    /// The encoded payload does not match the freshly resolved registration.
    Payload(MessagePayloadError),
    /// The tracked recipient failed validation before native transmission.
    StaleRecipient,
    /// Observed lifecycle invalidation. No native cleanup is attempted.
    Interrupted,
}

impl fmt::Display for MessageSendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy => f.write_str("an SDK message send is already in progress"),
            Self::Unavailable => f.write_str("native messaging is unavailable"),
            Self::Lookup(error) => error.fmt(f),
            Self::Payload(error) => error.fmt(f),
            Self::StaleRecipient => f.write_str("message recipient is stale"),
            Self::Interrupted => f.write_str("native message send was interrupted"),
        }
    }
}

impl std::error::Error for MessageSendError {}

impl<'callback> crate::Context<'callback> {
    /// Opt into a host contract for sends during this callback.
    ///
    /// # Safety
    /// For every send through this sender, the engine must be idle when begin
    /// is invoked, and all three native callbacks must preserve message ownership,
    /// recipient identity/allocation and map lifetime until end returns. Extensions and
    /// other plugins must not inject writes, end the message, or retain borrowed
    /// inputs. Native message hooks must accept byte-wise encoding rather than
    /// depending on original short/long/string writer boundaries. Establish
    /// this for the deployed engine and callback phase.
    ///
    /// Every supplied name/payload must match the receiving client's protocol,
    /// and recipients must be ready to parse it. Byte-length validation alone
    /// cannot establish these requirements. The caller must constrain all uses
    /// of the returned sender accordingly. This opt-in does not verify the host.
    pub unsafe fn message_sender(&self) -> MessageSender<'_, 'callback> {
        MessageSender { context: self }
    }
}

impl MessageSender<'_, '_> {
    /// Resolve the registration and recipient again, then send reliably to one player.
    ///
    /// Success means the native sequence returned, with no acknowledgement of delivery.
    /// No user closures or Rust formatting execute between begin and end.
    pub fn send_to(
        &self,
        name: &CStr,
        payload: &UserMessagePayload,
        player: crate::PlayerId,
    ) -> Result<(), MessageSendError> {
        let _guard = SendGuard::acquire()?;
        let map = crate::trace::active_map().ok_or(MessageSendError::Unavailable)?;
        let engine = crate::__ffi::engine().ok_or(MessageSendError::Unavailable)?;
        let (Some(begin), Some(write), Some(end)) =
            (engine.message_begin, engine.write_byte, engine.message_end)
        else {
            return Err(MessageSendError::Unavailable);
        };
        let message = self
            .context
            .metamod()
            .user_message(name)
            .map_err(MessageSendError::Lookup)?;
        message
            .validate_payload(payload)
            .map_err(MessageSendError::Payload)?;
        let edict = self
            .context
            .resolve_message_recipient(player)
            .map_err(|_| MessageSendError::StaleRecipient)?;
        let check = || {
            if crate::trace::active_map() == Some(map) && self.context.is_player_current(player) {
                Ok(())
            } else {
                Err(MessageSendError::Interrupted)
            }
        };
        check()?;
        // SAFETY: The opt-in establishes idle entry, valid recipient allocation,
        // synchronous input lifetimes and exclusive ownership through native end.
        // Registration and session were just validated. MSG_ONE is reliable unicast.
        unsafe { begin(1, i32::from(message.id), std::ptr::null(), edict) };
        check()?;
        for &byte in payload.as_bytes() {
            // SAFETY: The opted-in host preserves this native message and recipient.
            unsafe { write(i32::from(byte)) };
            check()?;
        }
        // SAFETY: The same host contract preserves ownership through end.
        unsafe { end() };
        check()
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::CString;

    use super::*;

    #[test]
    fn send_guard_releases_after_rust_unwind() {
        let _ = std::panic::catch_unwind(|| {
            let _guard = SendGuard::acquire().unwrap();
            assert!(matches!(SendGuard::acquire(), Err(MessageSendError::Busy)));
            panic!("controlled Rust unwind outside native calls");
        });
        assert!(SendGuard::acquire().is_ok());
    }

    #[test]
    fn rejects_invalid_ids_and_sizes_and_checks_registered_payload_size() {
        for (id, size) in [(-1, 0), (63, 0), (256, 0), (64, -2), (64, 193)] {
            assert_eq!(
                RegisteredUserMessage::from_native(id, size),
                Err(MessageLookupError::InvalidRegistration)
            );
        }
        let fixed = RegisteredUserMessage::from_native(64, 192).unwrap();
        let variable = RegisteredUserMessage::from_native(255, -1).unwrap();
        let mut payload = UserMessagePayload::new();
        assert_eq!(
            fixed.validate_payload(&payload),
            Err(MessagePayloadError::SizeMismatch {
                expected: 192,
                actual: 0,
            })
        );
        assert_eq!(variable.validate_payload(&payload), Ok(()));
        payload
            .write_string(&CString::new([1; 191]).unwrap())
            .unwrap();
        assert_eq!(fixed.validate_payload(&payload), Ok(()));
        assert_eq!(variable.validate_payload(&payload), Ok(()));
    }

    #[test]
    fn encodes_integer_fields_and_copies_native_strings() {
        let mut payload = UserMessagePayload::default();
        let string = CString::new([0xff]).unwrap();
        payload.write_byte(255).unwrap();
        payload.write_char(-128).unwrap();
        payload.write_short(-2).unwrap();
        payload.write_long(0x0102_0304).unwrap();
        payload.write_string(&string).unwrap();
        drop(string);
        payload.write_string(c"").unwrap();

        assert_eq!(
            payload.as_bytes(),
            &[255, 128, 254, 255, 4, 3, 2, 1, 255, 0, 0]
        );
        assert_eq!(payload.validate_fixed_size(11), Ok(()));
        assert_eq!(
            payload.validate_fixed_size(12),
            Err(MessagePayloadError::SizeMismatch {
                expected: 12,
                actual: 11
            })
        );
    }

    #[test]
    fn inclusive_limit_counts_nul_and_failed_fields_write_nothing() {
        let mut payload = UserMessagePayload::new();
        let data = CString::new([b'x'; MAX_USER_MESSAGE_BYTES - 2]).unwrap();
        payload.write_string(&data).unwrap();
        assert_eq!(payload.write_short(123), Err(MessagePayloadError::TooLong));
        assert_eq!(payload.as_bytes(), data.as_bytes_with_nul());
        payload.write_string(c"").unwrap();
        assert_eq!(payload.as_bytes().len(), MAX_USER_MESSAGE_BYTES);
        assert_eq!(payload.write_byte(1), Err(MessagePayloadError::TooLong));
        assert_eq!(payload.as_bytes()[MAX_USER_MESSAGE_BYTES - 2..], [0, 0]);

        let oversized = CString::new([b'x'; MAX_USER_MESSAGE_BYTES]).unwrap();
        let mut empty = UserMessagePayload::new();
        assert_eq!(
            empty.write_string(&oversized),
            Err(MessagePayloadError::TooLong)
        );
        assert!(empty.as_bytes().is_empty());
        assert_eq!(empty.validate_fixed_size(0), Ok(()));
    }
}
