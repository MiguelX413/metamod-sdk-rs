//! Fake-client setup for stock Ricochet on the tested hosts.

#![deny(unsafe_op_in_unsafe_fn)]

use metamod_sdk_rs::{FakeClient, FakeClientError, Server};
use std::ffi::CStr;

/// Stock Ricochet setup. See [`Self::new`] for the required host contract.
pub struct Ricochet {
    _private: (),
}

impl Ricochet {
    /// Opt into the stock Ricochet GameDLL contract.
    ///
    /// # Safety
    /// For every use of this adapter, the active deployment must implement the
    /// allocation, original GameDLL setup and queued kick contracts documented
    /// in the fake-client lifecycle guide. In particular, it must retain valid
    /// callback storage, allow the documented setup sequence, and avoid
    /// reassigning user IDs that could still be targeted by a queued kick.
    /// The tested configurations are Linux i686 Valve HLDS build 8684 and
    /// ReHLDS 3.15.0.896-dev with classic Metamod 1.21.1-am and stock Ricochet.
    pub unsafe fn new() -> Self {
        Self { _private: () }
    }

    /// Create a stock Ricochet fake client from an active server callback.
    /// Accepts 1–31 ASCII letters, digits or underscores.
    pub fn create_fake_client(
        &self,
        server: &Server<'_, '_>,
        name: &CStr,
    ) -> Result<FakeClient, FakeClientError> {
        let bytes = name.to_bytes();
        if !(1..=31).contains(&bytes.len())
            || !bytes
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            return Err(FakeClientError::InvalidName);
        }
        // SAFETY: The operator established this adapter's deployment contract.
        // Stock Ricochet accepts admission before put-in-server and clears the
        // engine's fake-client flag during spawn. Each step revalidates identity.
        unsafe {
            server.create_fake_client(name, |setup| {
                setup.connect(c"loopback") && setup.put_in_server() && setup.restore_fake_flag()
            })
        }
    }
}
