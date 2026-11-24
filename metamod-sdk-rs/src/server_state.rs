//! Owned observations of active-map engine globals.

use crate::{
    Server,
    engine::{MAX_ENGINE_STRING, StringRead, copy_string},
    trace::{active_globals, active_map, validated_capacities},
};
use std::{ffi::CString, ptr};

/// Owned timing and game-state values from an active map.
///
/// Values can change after this call. Keeping a snapshot does not keep its map
/// active. Scalars are copied before resolving either string, with no native
/// call between scalar reads.
/// Trace scratch fields, private pointers, and mutable direction vectors are
/// outside this snapshot.
#[derive(Clone, Debug, PartialEq)]
pub struct ServerState {
    /// Native engine/map simulation time in seconds, or `None` for NaN/infinity.
    ///
    /// This is an `f32` clock, not wall time, a Unix timestamp, or a monotonic
    /// process timer. It can reset or jump on map changes/load and lose precision.
    /// Finite negative values are preserved rather than clamped.
    pub time: Option<f32>,
    /// Native frame duration in seconds. Zero is valid; negative or non-finite
    /// values are `None`. It is not a promised tick interval or measured CPU time.
    pub frame_time: Option<f32>,
    /// Current advertised client capacity, validated against `max_entities`.
    /// `None` means nonpositive, inconsistent, or unavailable entity capacity.
    /// This is capacity rather than a count of connected players.
    pub max_clients: Option<i32>,
    /// Current advertised edict capacity, including the world slot.
    /// Values outside the SDK's supported 1..=65,536 range are `None`.
    /// Entity resolution uses its separate post-activation capacity snapshot.
    pub max_entities: Option<i32>,
    /// Owned native map name. Zero index, missing lookup, null, or strings that
    /// exceed 4,096 bytes including NUL become `None`. Empty strings stay empty.
    pub map_name: Option<CString>,
    /// Owned native starting-spot string, with the same absence rules as
    /// `map_name`. Interpretation is GameDLL-defined.
    pub start_spot: Option<CString>,
    /// Finite native deathmatch indicator. Zero is conventionally off; other
    /// values, including negative or fractional modes, remain uninterpreted.
    pub deathmatch: Option<f32>,
    /// Finite native cooperative-play indicator, without a GameDLL-specific
    /// interpretation. NaN/infinity become `None`.
    pub coop: Option<f32>,
    /// Finite native teamplay indicator. This does not establish teams or
    /// membership, or promise that the GameDLL uses this field.
    pub teamplay: Option<f32>,
    /// Finite native server/game flags stored as a float in the ABI.
    /// No universal bit layout or integer conversion is imposed.
    pub server_flags: Option<f32>,
}

fn finite(value: f32) -> Option<f32> {
    value.is_finite().then_some(value)
}

impl Server<'_, '_> {
    /// Copy timing, capacities, mode indicators, and map strings from engine globals.
    ///
    /// Available only after outermost post-activation and before deactivation
    /// or shutdown, on the server thread. Returns `None` if globals are missing
    /// or a string lookup is observably interrupted by SDK reentry or map change.
    /// Invalid individual values are represented by `None` fields.
    ///
    /// Native strings must remain readable through their NUL or the read limit
    /// and must survive activity the SDK cannot observe. Copies complete before
    /// another native call.
    ///
    ///     use metamod_sdk_rs::Context;
    ///
    ///     fn map_time(ctx: &Context<'_>) -> Option<f32> {
    ///         ctx.server().state()?.time
    ///     }
    pub fn state(&self) -> Option<ServerState> {
        let (map, globals) = active_globals()?;
        // SAFETY: The loader retains aligned globals, and the active-map host
        // contract initializes these scalar fields by post-activation. This is
        // the owner thread. Copy only selected fields, with no foreign references
        // and no native calls that could reenter or invalidate the globals.
        let (mut state, names) = unsafe {
            let entities = ptr::addr_of!((*globals).maxEntities).read();
            let clients = ptr::addr_of!((*globals).maxClients).read();
            let (max_entities, max_clients) = validated_capacities(entities, clients);
            let frame_time = ptr::addr_of!((*globals).frametime).read();
            (
                ServerState {
                    time: finite(ptr::addr_of!((*globals).time).read()),
                    frame_time: finite(frame_time).filter(|value| *value >= 0.0),
                    max_clients,
                    max_entities,
                    map_name: None,
                    start_spot: None,
                    deathmatch: finite(ptr::addr_of!((*globals).deathmatch).read()),
                    coop: finite(ptr::addr_of!((*globals).coop).read()),
                    teamplay: finite(ptr::addr_of!((*globals).teamplay).read()),
                    server_flags: finite(ptr::addr_of!((*globals).serverflags).read()),
                },
                [
                    ptr::addr_of!((*globals).mapname).read(),
                    ptr::addr_of!((*globals).startspot).read(),
                ],
            )
        };
        if let Some(lookup) = crate::__ffi::engine()?.string_from_index {
            for (index, output) in names
                .into_iter()
                .zip([&mut state.map_name, &mut state.start_spot])
            {
                if index == 0 {
                    continue;
                }
                let reading = StringRead::begin()?;
                // SAFETY: The captured string_t belongs to the current map.
                // The host preserves string indices through that map's lifetime.
                let string = unsafe { lookup(index as i32) };
                if !reading.uninterrupted() || active_map() != Some(map) {
                    return None;
                }
                // SAFETY: The host returns a readable string for this map, or
                // null. No native call follows until this bounded copy is owned.
                *output = unsafe { copy_string(string, MAX_ENGINE_STRING) };
            }
        }
        (active_map() == Some(map)).then_some(state)
    }
}
