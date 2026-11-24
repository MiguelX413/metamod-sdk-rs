//! Macro-facing entry points and crate-internal runtime access.

pub use metamod_sys::*;

mod callbacks;
mod dispatch;
mod hook_tables;
mod lifecycle;
mod players;
mod state;

pub use lifecycle::{attach, capture_game_dll, give_engine, info, query};
pub(crate) use players::{player_at, player_session, player_slot, track_fake_client};
pub(crate) use state::{engine, engine_available, registration_available};

#[cfg(test)]
mod tests;
