//! Owned construction of GoldSrc user-message payloads.

use std::{ffi::CStr, fmt};

/// Maximum user-message payload size in bytes, excluding protocol framing.
pub const MAX_USER_MESSAGE_BYTES: usize = 192;

/// A user-message payload assembled from GoldSrc wire fields.
///
/// Construction is independent of the server and makes no engine calls.
/// The message's protocol, registered size and recipient must be checked
/// separately before native transmission.
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
    TooLong,
    SizeMismatch { expected: usize, actual: usize },
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

    /// Verify an expected fixed payload size supplied by the caller.
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
    pub fn write_string(&mut self, value: &CStr) -> Result<(), MessagePayloadError> {
        self.append(value.to_bytes_with_nul())
    }

    // Check the whole field before modifying the payload.
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

#[cfg(test)]
mod tests {
    use std::ffi::CString;

    use super::*;

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
