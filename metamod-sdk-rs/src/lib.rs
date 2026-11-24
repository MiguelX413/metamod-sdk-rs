//! Build native GoldSrc Metamod plugins with Rust state and typed hooks.
//!
//! Implement [`Plugin`] to receive server callbacks. Engine-facing callbacks
//! receive a scoped [`Context`] for engine and Metamod services; shutdown runs
//! without engine access. The SDK owns one plugin instance on the server thread
//! and contains unwinding Rust panics at its FFI boundary. Abort-on-panic builds
//! still terminate the process.
//!
//! # Quick start
//!
//! Build your plugin as a `cdylib`. The default `macros` feature supplies the
//! registration attribute:
//!
//! ```toml
//! [lib]
//! crate-type = ["cdylib"]
//!
//! [dependencies]
//! metamod-sdk-rs = { git = "https://github.com/MiguelX413/metamod-sdk-rs" }
//! ```
//!
//! ```
//! # #[cfg(feature = "macros")]
//! # mod example {
//! use metamod_sdk_rs::prelude::*;
//!
//! #[derive(Default)]
//! struct MyPlugin;
//!
//! #[metamod_plugin(
//!     name = "My Plugin",
//!     version = "0.1.0",
//!     author = "Your Name",
//!     log_tag = "MYPLUGIN"
//! )]
//! impl Plugin for MyPlugin {
//!     fn server_activate(&mut self, ctx: &mut Context<'_>) {
//!         ctx.server().print(c"My Plugin: map activated\n");
//!     }
//! }
//! # }
//! ```
//!
//! # Find an API
//!
//! | Task | Start here |
//! | --- | --- |
//! | Handle frames, maps, admission, and player events | [`Plugin`] |
//! | Print, register commands, and access CVars | [`Context::server`], [`Server`], [`Command`] |
//! | Log through Metamod | [`Context::metamod`], [`MetaMod`] |
//! | Track player sessions | [`PlayerId`], [`Context::player_at`] |
//! | Discover entities and copy their properties | [`EntityHandle`], [`EntityInfo`], [`EntitySpatial`] |
//! | Trace geometry | [`TraceSegment`], [`TraceOptions`], [`TraceResult`] |
//! | Prepare user-message bytes | [`UserMessagePayload`] |
//! | Opt into native user-message delivery | [`MessageSender`] and its unsafe host contract |
//! | Implement game-specific fake-client setup | [`FakeClientSetup`] |
//!
//! [`prelude`] collects the common imports. Errors such as [`EngineError`],
//! [`EntityError`], and [`MessageSendError`] describe unavailable operations and
//! invalidated identities; native calls can synchronously change lifecycle state.
//!
//! # Registration without procedural macros
//!
//! Set `default-features = false` on the dependency, implement
//! [`Plugin::METADATA`], and invoke [`export_plugin!`] once per plugin library.
//! This uses the same runtime and hooks as attribute registration.
//!
//! ```
//! use metamod_sdk_rs::{Metadata, Plugin, export_plugin};
//!
//! #[derive(Default)]
//! struct MyPlugin;
//!
//! impl Plugin for MyPlugin {
//!     const METADATA: Metadata = Metadata {
//!         name: c"My Plugin",
//!         version: c"0.1.0",
//!         date: c"",
//!         author: c"Your Name",
//!         url: c"",
//!         log_tag: c"MYPLUGIN",
//!     };
//! }
//!
//! export_plugin!(MyPlugin);
//! ```
//!
//! # Lifecycle and native safety
//!
//! Plugins load at server startup only. Runtime loading and unloading are
//! unsupported; stop the server before replacing a plugin. The SDK refuses
//! detach and keeps command registrations and bounded CVar storage resident.
//!
//! Keep plugin state on the server thread. Engine calls can synchronously
//! reenter the SDK; nested notifications are deferred while plugin state is
//! mutably borrowed. Callback contexts and native pointers must not escape
//! their documented lifetimes. Player sessions and entity identities can expire
//! across native calls or map changes; use the SDK's validation operations.
//!
//! The native host must uphold pointer, string, allocation, callback, and
//! ownership contracts. Null and alignment checks cannot establish arbitrary
//! pointer validity. Native message sending additionally requires explicit
//! unsafe host opt-in; payload construction alone does not establish safe
//! delivery or client protocol correctness.
//!
//! # Build, examples, and verification
//!
//! Native deployments target Linux i686 or Windows x86. Building requires the
//! pinned upstream headers and the target's C/C++ toolchain; a host documentation
//! build does not verify a native deployment. Live compatibility and memory
//! evidence have separate, configuration-specific scopes.
//!
//! - [SDK reference and installation](https://github.com/MiguelX413/metamod-sdk-rs/blob/master/docs/sdk-reference.md)
//! - [Example plugins](https://github.com/MiguelX413/metamod-sdk-rs/tree/master/examples)
//! - [API coverage and roadmap](https://github.com/MiguelX413/metamod-sdk-rs/blob/master/docs/api-coverage.md)
//! - [Native memory contracts and evidence](https://github.com/MiguelX413/metamod-sdk-rs/blob/master/docs/memory-safety-audit.md)
#![deny(unsafe_op_in_unsafe_fn)]

use std::ffi::CStr;

#[doc(hidden)]
pub mod __ffi;
mod engine;
mod hooks;
mod message;
mod runtime;
mod trace;

#[cfg(all(test, metamod_lsan))]
fn ignore_intentionally_retained<T: ?Sized>(allocation: &T) {
    unsafe extern "C" {
        fn __lsan_ignore_object(pointer: *const std::ffi::c_void);
    }

    // SAFETY: The sanitizer test build links LeakSanitizer. The reference
    // identifies the live allocation explicitly retained by the caller.
    unsafe { __lsan_ignore_object(std::ptr::from_ref(allocation).cast()) };
}

mod fake_client;
pub use fake_client::{FakeClient, FakeClientError, FakeClientSetup, FakeClientState};

pub use engine::{Command, ConsoleName, CvarValue, EngineError, OwnedConsoleName, PlayerInfo};
pub use hooks::{ClientAdmission, ClientConnection, HookResult, KeyValueEvent, PostHookResult};
pub use message::{
    MAX_USER_MESSAGE_BYTES, MessageLookupError, MessagePayloadError, MessageRecipient,
    MessageSendError, MessageSender, RegisteredUserMessage, StaleMessageRecipient,
    UserMessagePayload, UserMessageSize,
};
pub use trace::{
    EntityError, EntityHandle, EntityInfo, EntityKeyValues, EntityKind, EntitySpatial, Hull,
    TraceError, TraceMonsters, TraceOptions, TracePlane, TraceResult, TraceSegment,
};

pub use runtime::{
    Context, InvalidPlayerSlot, InvalidPlayerUserId, MetaMod, MetamodMessage,
    MetamodMessageTooLong, ParsePlayerNumberError, PlayerId, PlayerSlot, PlayerUserId, Server,
};

#[cfg(feature = "macros")]
pub use metamod_macros::metamod_plugin;

/// Common types and registration helpers for plugin implementations.
pub mod prelude {
    #[cfg(feature = "macros")]
    pub use crate::metamod_plugin;
    pub use crate::{
        ClientAdmission, ClientConnection, Command, ConsoleName, Context, CvarValue, EngineError,
        EntityError, EntityHandle, EntityInfo, EntityKeyValues, EntityKind, EntitySpatial,
        HookResult, Hull, KeyValueEvent, MAX_USER_MESSAGE_BYTES, MessageLookupError,
        MessagePayloadError, MessageRecipient, MessageSendError, MessageSender, MetaMod, Metadata,
        MetamodMessage, MetamodMessageTooLong, OwnedConsoleName, PlayerId, PlayerInfo, PlayerSlot,
        PlayerUserId, Plugin, PostHookResult, RegisteredUserMessage, Server, StaleMessageRecipient,
        TraceError, TraceMonsters, TraceOptions, TracePlane, TraceResult, TraceSegment,
        UserMessagePayload, UserMessageSize, console_name, cvar_value, export_plugin,
    };
}

/// Validate and lowercase a constant C string as a console name.
///
/// ```
/// use metamod_sdk_rs::{ConsoleName, console_name};
/// static NAME: &ConsoleName = console_name!(c"MY_PLUGIN");
/// assert_eq!(NAME.as_c_str(), c"my_plugin");
/// ```
///
/// ```compile_fail
/// use metamod_sdk_rs::console_name;
/// let _ = console_name!(c"bad name");
/// ```
#[macro_export]
macro_rules! console_name {
    ($name:expr $(,)?) => {
        const {
            const INPUT: &::core::ffi::CStr = $name;
            const LEN: usize = INPUT.to_bytes_with_nul().len();
            const BYTES: [u8; LEN] = $crate::ConsoleName::__normalize::<LEN>(INPUT);
            $crate::ConsoleName::__from_static_bytes(&BYTES)
        }
    };
}

/// Validate a constant C string against the CVar byte limit.
///
/// ```
/// use metamod_sdk_rs::{CvarValue, cvar_value};
/// const VALUE: &CvarValue = cvar_value!(c"\xff");
/// assert_eq!(VALUE.as_c_str().to_bytes(), &[0xff]);
/// ```
///
/// ```compile_fail
/// use metamod_sdk_rs::cvar_value;
/// const BYTES: [u8; 1025] = {
///     let mut bytes = [b'x'; 1025];
///     bytes[1024] = 0;
///     bytes
/// };
/// const VALUE: &core::ffi::CStr = core::ffi::CStr::from_bytes_with_nul(&BYTES).unwrap();
/// let _ = cvar_value!(VALUE);
/// ```
#[macro_export]
macro_rules! cvar_value {
    ($value:expr $(,)?) => {
        const {
            match $crate::CvarValue::new($value) {
                Ok(value) => value,
                Err(_) => panic!("CVar value exceeds 1023 bytes"),
            }
        }
    };
}

/// Strings remain available to Metamod for the lifetime of the module.
pub struct Metadata {
    pub name: &'static CStr,
    pub version: &'static CStr,
    pub date: &'static CStr,
    pub author: &'static CStr,
    pub url: &'static CStr,
    pub log_tag: &'static CStr,
}

/// One server-thread-owned instance per successful startup attachment.
pub trait Plugin: Default + 'static {
    const METADATA: Metadata;

    /// Return false to decline attachment and drop this instance.
    fn initialize(&mut self, _context: &mut Context<'_>) -> bool {
        true
    }

    /// Best-effort cleanup when Metamod forwards the game-DLL shutdown hook.
    /// Cleanup waits for any active plugin callback to return, then the SDK drops
    /// this instance. Engine access is unavailable.
    fn shutdown(&mut self) {}

    /// Handle a command registered through `Server::register_command`.
    fn server_command(&mut self, _context: &mut Context<'_>, _command: &Command) {}

    /// Opt into the synchronous GameDLL client admission pre-hook.
    const CLIENT_CONNECT_PRE: bool = false;

    /// Decide whether to reject a connection before the GameDLL sees it.
    /// Nested callbacks, invalid inputs, and panics defer to the host.
    fn client_connect(
        &mut self,
        _context: &mut Context<'_>,
        _client: &ClientConnection,
    ) -> ClientAdmission {
        ClientAdmission::Defer
    }

    /// Register the pre and post lag compensation hooks independently.
    const LAG_COMPENSATION_PRE: bool = false;
    const LAG_COMPENSATION_POST: bool = false;

    fn allow_lag_compensation(&mut self, _context: &mut Context<'_>) -> HookResult<bool> {
        HookResult::Ignored
    }

    /// Metamod's reported original result. A superceded call has no original.
    fn allow_lag_compensation_post(
        &mut self,
        _context: &mut Context<'_>,
        _original: Option<bool>,
    ) -> PostHookResult<bool> {
        PostHookResult::Ignored
    }

    /// Opt into observing GameDLL keyvalue inputs, including before map activation.
    const KEYVALUE_PRE: bool = false;

    /// Observe copied inputs. Reentrant observations may be delivered later.
    fn key_value(&mut self, _context: &mut Context<'_>, _event: &KeyValueEvent) {}

    fn start_frame(&mut self, _context: &mut Context<'_>) {}

    /// Called after the game DLL activates the map.
    fn server_activate(&mut self, _context: &mut Context<'_>) {}

    /// Called before the game DLL deactivates the map.
    fn server_deactivate(&mut self, _context: &mut Context<'_>) {}

    /// Observes a session after the game DLL puts the player into the server.
    fn player_join(&mut self, _context: &mut Context<'_>, _player: PlayerId) {}

    /// Observes a session ending. The ID is already invalidated.
    fn player_disconnect(&mut self, _context: &mut Context<'_>, _player: PlayerId) {}
}

/// Put this once at the root of the plugin's cdylib crate.
///
/// ```
/// use metamod_sdk_rs::prelude::*;
///
/// #[derive(Default)]
/// struct MyPlugin;
///
/// impl Plugin for MyPlugin {
///     const METADATA: Metadata = Metadata {
///         name: c"My Plugin",
///         version: c"0.1.0",
///         author: c"Developer",
///         date: c"",
///         url: c"",
///         log_tag: c"MYPLUGIN",
///     };
/// }
///
/// export_plugin!(MyPlugin);
/// ```
#[macro_export]
macro_rules! export_plugin {
    ($plugin:ty) => {
        use $crate::__ffi as __metamod_ffi;

        static mut __METAMOD_INFO: __metamod_ffi::PluginInfo = __metamod_ffi::info::<$plugin>();

        #[cfg(all(windows, target_arch = "x86"))]
        #[unsafe(no_mangle)]
        unsafe extern "stdcall" fn GiveFnptrsToDll(
            engine: *mut __metamod_ffi::EngineFunctions,
            globals: *mut __metamod_ffi::EngineGlobals,
        ) {
            // SAFETY: The loader supplies the engine function table and globals.
            unsafe { __metamod_ffi::give_engine(engine, globals) };
        }

        #[cfg(not(all(windows, target_arch = "x86")))]
        #[unsafe(no_mangle)]
        unsafe extern "C" fn GiveFnptrsToDll(
            engine: *mut __metamod_ffi::EngineFunctions,
            globals: *mut __metamod_ffi::EngineGlobals,
        ) {
            // SAFETY: The loader supplies the engine function table and globals.
            unsafe { __metamod_ffi::give_engine(engine, globals) };
        }

        #[unsafe(no_mangle)]
        unsafe extern "C" fn Meta_Query(
            version: *const ::core::ffi::c_char,
            out: *mut *mut __metamod_ffi::PluginInfo,
            utilities: *mut __metamod_ffi::MetaUtilities,
        ) -> ::core::ffi::c_int {
            // SAFETY: The loader supplies the C ABI arguments.
            unsafe { __metamod_ffi::query(version, out, utilities, &raw mut __METAMOD_INFO) }
        }

        #[unsafe(no_mangle)]
        unsafe extern "C" fn Meta_Attach(
            now: __metamod_ffi::LoadTime,
            table: *mut __metamod_ffi::MetaFunctions,
            globals: *mut __metamod_ffi::MetaGlobals,
            game: *mut __metamod_ffi::GameDllFunctions,
        ) -> ::core::ffi::c_int {
            // SAFETY: The loader supplies the C ABI arguments.
            unsafe {
                let attached = __metamod_ffi::attach::<$plugin>(now, table, globals);
                if attached != 0 {
                    __metamod_ffi::capture_game_dll(game);
                }
                attached
            }
        }

        #[unsafe(no_mangle)]
        unsafe extern "C" fn Meta_Detach(
            _: __metamod_ffi::LoadTime,
            _: __metamod_ffi::UnloadReason,
        ) -> ::core::ffi::c_int {
            0
        }

        // Check exported signatures against bindgen's upstream declarations.
        const _: __metamod_ffi::raw::GIVE_ENGINE_FUNCTIONS_FN = Some(GiveFnptrsToDll);
        const _: __metamod_ffi::raw::META_QUERY_FN = Some(Meta_Query);
        const _: __metamod_ffi::raw::META_ATTACH_FN = Some(Meta_Attach);
        const _: __metamod_ffi::raw::META_DETACH_FN = Some(Meta_Detach);
    };
}
