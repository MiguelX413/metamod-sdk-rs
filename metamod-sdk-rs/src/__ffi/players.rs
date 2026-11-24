//! Session lookup and reentry-aware native slot resolution.

use super::state::{ENGINE, EVENTS, INDEX_LOOKUP, SHUTDOWN, engine_available};
use crate::{PlayerId, PlayerSlot};
use metamod_sys::raw::edict_t;

pub(crate) fn player_at(slot: PlayerSlot) -> Option<PlayerId> {
    player_session(slot).map(|(id, _)| id)
}

pub(crate) fn player_session(slot: PlayerSlot) -> Option<(PlayerId, i32)> {
    if !engine_available() {
        return None;
    }
    EVENTS.with(|events| events.borrow().players.get(&slot).copied())
}

pub(crate) fn track_fake_client(slot: PlayerSlot, serial: i32) -> Option<PlayerId> {
    EVENTS.with(|events| {
        let mut events = events.borrow_mut();
        if events.players.contains_key(&slot) {
            return None;
        }
        events.join(slot, serial)
    })
}

pub(super) fn reset_players() {
    crate::fake_client::invalidate();
    if INDEX_LOOKUP.get().is_some() {
        INDEX_LOOKUP.set(Some(true));
    }
    EVENTS.with(|events| events.borrow_mut().players.clear());
}

/// # Safety
/// A non-null aligned edict must be valid for the native index lookup.
/// Its identity fields must remain readable until a map transition or shutdown.
pub(crate) unsafe fn player_slot(edict: *mut edict_t) -> Option<(PlayerSlot, i32)> {
    let index = ENGINE.get().index?;
    if SHUTDOWN.get() {
        return None;
    }
    if INDEX_LOOKUP.get().is_some() {
        // A hook reentered slot resolution through an engine wrapper. The
        // outer edict may no longer identify its original session.
        reset_players();
        return None;
    }
    // SAFETY: The hook supplies the edict and its initialized identity fields.
    let serial = unsafe { crate::trace::live_serial(edict) }?;
    INDEX_LOOKUP.set(Some(false));
    // SAFETY: Metamod supplied a live edict for this synchronous lookup.
    let index = unsafe { index(edict) };
    if INDEX_LOOKUP.replace(None) == Some(false)
        && crate::trace::within_client_capacity(index)
        // SAFETY: No observed map transition or shutdown retired the slot.
        && unsafe { crate::trace::live_serial(edict) } == Some(serial)
    {
        PlayerSlot::new(index).map(|slot| (slot, serial))
    } else {
        None
    }
}
