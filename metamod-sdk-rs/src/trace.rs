use crate::{
    __ffi, Context, Server,
    engine::{Engine, MAX_ENGINE_STRING, StringRead, copy_string},
};
use metamod_sys::{
    EngineGlobals,
    raw::{self, edict_t},
};
use std::{cell::Cell, ffi::CString, fmt, ptr};

// SDK work limit, independent of the lower capacity a host may advertise.
const MAX_EDICT_CAPACITY: i32 = 65_536;

thread_local! {
    // Keep the counter across deactivation so a new map cannot revive handles.
    static GLOBALS: Cell<*mut EngineGlobals> = const { Cell::new(ptr::null_mut()) };
    static LIMIT: Cell<i32> = const { Cell::new(0) };
    static CLIENT_LIMIT: Cell<i32> = const { Cell::new(0) };
    static MAP: Cell<(u64, bool)> = const { Cell::new((0, false)) };
}

/// # Safety
/// Non-null globals must be aligned and kept allocated through shutdown.
/// The host must initialize both capacities before each
/// post-activation hook and confine accesses to the server thread.
pub(crate) unsafe fn set_globals(globals: *mut EngineGlobals) {
    GLOBALS.set(globals);
}

pub(crate) fn invalidate_map() {
    LIMIT.set(0);
    CLIENT_LIMIT.set(0);
    let (generation, _) = MAP.get();
    MAP.set((generation, false));
}

pub(crate) fn activate_map() {
    invalidate_map();
    let (generation, _) = MAP.get();
    if let Some(generation) = generation.checked_add(1) {
        MAP.set((generation, true));
        let globals = GLOBALS.get();
        if !globals.is_null() {
            // SAFETY: The loader retains initialized globals and this is the
            // owner-thread post-activation hook for the current map.
            let (capacity, clients) = unsafe {
                (
                    ptr::addr_of!((*globals).maxEntities).read(),
                    ptr::addr_of!((*globals).maxClients).read(),
                )
            };
            if (1..=MAX_EDICT_CAPACITY).contains(&capacity) {
                LIMIT.set(capacity);
                if (1..capacity).contains(&clients) {
                    CLIENT_LIMIT.set(clients);
                }
            }
        }
    } else {
        invalidate_map();
    }
}

pub(crate) fn active_map() -> Option<u64> {
    let (generation, active) = MAP.get();
    (active && __ffi::engine_available()).then_some(generation)
}

/// Identifies a live edict allocation within one map. The world edict
/// can have index zero. Keep the entire handle for identity comparisons.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct EntityHandle {
    index: i32,
    serial: i32,
    map: u64,
}

impl EntityHandle {
    /// Diagnostic engine index. It can be reused after entity removal.
    pub fn index(self) -> i32 {
        self.index
    }

    /// # Safety
    /// The pointer must have been resolved for this handle on the current
    /// thread. On the same map its edict slot must remain allocated even
    /// if the entity occupying it has been freed or replaced.
    pub(crate) unsafe fn still_matches(self, edict: *mut edict_t) -> bool {
        active_map() == Some(self.map)
            // SAFETY: The epoch guards the edict arena lifetime.
            && unsafe { live_serial(edict) } == Some(self.serial)
    }

    pub(crate) fn resolve(self, engine: Engine) -> Option<*mut edict_t> {
        let entity = engine.entity?;
        if active_map() != Some(self.map) || !valid_index(self.index) {
            return None;
        }
        // SAFETY: The index originated from this map's engine edict table.
        let edict = unsafe { entity(self.index) };
        if active_map() != Some(self.map) {
            return None;
        }
        // SAFETY: The getter returned a current-map edict or null.
        (unsafe { live_serial(edict) } == Some(self.serial)).then_some(edict)
    }
}

fn valid_index(index: i32) -> bool {
    (0..LIMIT.get()).contains(&index)
}

pub(crate) fn within_client_capacity(index: i32) -> bool {
    // Early player hooks can precede the post-activation capacity snapshot.
    let capacity = CLIENT_LIMIT.get();
    capacity == 0 || index <= capacity
}

pub(crate) fn tracked_entity(index: i32, serial: i32) -> Option<EntityHandle> {
    let map = active_map()?;
    (1..=CLIENT_LIMIT.get())
        .contains(&index)
        .then_some(EntityHandle { index, serial, map })
}

/// # Safety
/// For a non-null aligned edict, the host must keep its free and serial
/// fields initialized and readable for this immediate owner-thread read.
pub(crate) unsafe fn live_serial(edict: *mut edict_t) -> Option<i32> {
    if edict.is_null() || !edict.is_aligned() {
        return None;
    }
    // SAFETY: These integer fields are initialized in the engine's edict.
    unsafe { ((*edict).free == 0).then(|| (*edict).serialnumber) }
}

impl Context<'_> {
    /// Revalidate the map and native allocation generation of a saved handle.
    pub fn is_entity_current(&self, entity: EntityHandle) -> bool {
        __ffi::engine()
            .and_then(|engine| entity.resolve(engine))
            .is_some()
    }
}

/// Broad engine classification derived from world index and edict flags.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntityKind {
    /// The world edict at index zero.
    World,
    /// An entity carrying the engine client flag without the fake-client flag.
    Client,
    /// An entity carrying the engine fake-client flag, even if its client flag
    /// is absent. This category takes precedence over client and monster flags.
    FakeClient,
    /// An entity carrying the engine monster flag, after client classification.
    Monster,
    /// An entity not matched by the world, client, or monster categories.
    Other,
}

/// Owned string keyvalues retained in GoldSrc's common entvars fields.
/// Zero string indices and null engine strings are represented by `None`.
/// GameDLL-specific map keys may never have been stored in these fields.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EntityKeyValues {
    /// GameDLL classname, if retained in the common entvars field.
    pub classname: Option<CString>,
    /// Global entity name used by game-specific cross-map logic.
    pub globalname: Option<CString>,
    /// Target identifier referenced by this entity.
    pub target: Option<CString>,
    /// Name by which other entities may target this entity.
    pub targetname: Option<CString>,
    /// Model name or inline model reference as retained by the GameDLL.
    pub model: Option<CString>,
    /// Native network/display name, with game-specific meaning.
    pub netname: Option<CString>,
    /// Native message field; interpretation depends on the GameDLL.
    pub message: Option<CString>,
}

/// Entity classification and retained keyvalues copied at lookup time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntityInfo {
    /// Identity validated while copying this snapshot; it may expire later.
    pub entity: EntityHandle,
    /// Classification derived from the world index and native flags.
    pub kind: EntityKind,
    /// Owned copies of the common retained string fields.
    pub keyvalues: EntityKeyValues,
}

/// Owned spatial fields as reported by the engine at lookup time.
/// The snapshot remains readable after its entity handle expires.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EntitySpatial {
    /// Identity validated while copying the spatial fields.
    pub entity: EntityHandle,
    /// World-space origin in engine units.
    pub origin: [f32; 3],
    /// Native pitch, yaw, and roll in degrees.
    pub angles: [f32; 3],
    /// Native velocity in engine units per second.
    pub velocity: [f32; 3],
    /// Absolute world-space minimum of the engine-reported bounding box.
    pub bounds_min: [f32; 3],
    /// Absolute world-space maximum of the engine-reported bounding box.
    pub bounds_max: [f32; 3],
}

/// Entity discovery could not obtain a consistent active-map result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntityError {
    /// No active map, valid entity capacity, or required edict callback is available.
    Unavailable,
    /// Map retirement or shutdown invalidated discovery during a native call.
    MapChanged,
}

impl fmt::Display for EntityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unavailable => "entity discovery requires an active map and edict table",
            Self::MapChanged => "the map changed during entity discovery",
        })
    }
}

impl std::error::Error for EntityError {}

fn vector(v: raw::Vector) -> [f32; 3] {
    [v.x, v.y, v.z]
}

/// # Safety
/// If the map is unchanged on return, `lookup` must return either null
/// or an edict in that map with readable, initialized identity fields.
unsafe fn find_entity(
    lookup: impl FnOnce(i32) -> *mut edict_t,
    index: i32,
    map: u64,
) -> Result<Option<EntityHandle>, EntityError> {
    if active_map() != Some(map) {
        return Err(EntityError::MapChanged);
    }
    if !valid_index(index) {
        return Ok(None);
    }
    let edict = lookup(index);
    if active_map() != Some(map) {
        return Err(EntityError::MapChanged);
    }
    // SAFETY: The lookup supplied a current-map edict or null and no
    // other native call intervened.
    Ok(unsafe { live_serial(edict) }.map(|serial| EntityHandle { index, serial, map }))
}

impl Context<'_> {
    /// Discover a live engine-visible edict by index, including world index zero.
    /// Out-of-range and vacant indices return `Ok(None)`.
    pub fn entity_at(&self, index: i32) -> Result<Option<EntityHandle>, EntityError> {
        let (map, limit, lookup) = discovery()?;
        if !(0..limit).contains(&index) {
            return Ok(None);
        }
        // SAFETY: The engine implements the current-map edict lookup contract.
        unsafe { find_entity(|index| lookup(index), index, map) }
    }

    /// Scan the engine-visible edict table in index order. Each handle
    /// describes the allocation seen at its lookup and can expire later.
    /// A map transition interrupts the scan with `EntityError::MapChanged`.
    pub fn entities(&self) -> Result<Vec<EntityHandle>, EntityError> {
        let (map, limit, lookup) = discovery()?;
        let mut found = Vec::new();
        for index in 0..limit {
            // SAFETY: The engine implements the current-map edict lookup contract.
            if let Some(entity) = unsafe { find_entity(|index| lookup(index), index, map) }? {
                found.push(entity);
            }
        }
        Ok(found)
    }

    /// Copy engine classification and common string keyvalues. Returns
    /// `None` if lookup is unavailable or the allocation expires mid-read.
    ///
    /// The returned strings are owned. Keeping a snapshot does not keep the
    /// native entity alive; its handle must be revalidated for later operations.
    ///
    /// ```
    /// use metamod_sdk_rs::{Context, EntityError, EntityInfo};
    ///
    /// fn world_info(ctx: &Context<'_>) -> Result<Option<EntityInfo>, EntityError> {
    ///     let Some(world) = ctx.entity_at(0)? else { return Ok(None) };
    ///     // This second lookup can fail if a native call invalidates the handle.
    ///     Ok(ctx.entity_info(world))
    /// }
    /// ```
    pub fn entity_info(&self, entity: EntityHandle) -> Option<EntityInfo> {
        let engine = __ffi::engine()?;
        let lookup = engine.string_from_index?;
        let edict = entity.resolve(engine)?;

        // SAFETY: The edict has just been validated. Copy flags and offsets
        // before invoking any engine function that could invalidate it.
        let (flags, indices) = unsafe {
            let vars = ptr::addr_of!((*edict).v);
            (
                ptr::addr_of!((*vars).flags).read(),
                [
                    ptr::addr_of!((*vars).classname).read(),
                    ptr::addr_of!((*vars).globalname).read(),
                    ptr::addr_of!((*vars).target).read(),
                    ptr::addr_of!((*vars).targetname).read(),
                    ptr::addr_of!((*vars).model).read(),
                    ptr::addr_of!((*vars).netname).read(),
                    ptr::addr_of!((*vars).message).read(),
                ],
            )
        };
        let kind = if entity.index() == 0 {
            EntityKind::World
        } else if flags & raw::FL_FAKECLIENT as i32 != 0 {
            EntityKind::FakeClient
        } else if flags & raw::FL_CLIENT as i32 != 0 {
            EntityKind::Client
        } else if flags & raw::FL_MONSTER as i32 != 0 {
            EntityKind::Monster
        } else {
            EntityKind::Other
        };
        let mut strings = [const { None }; 7];
        for (index, value) in indices.into_iter().zip(&mut strings) {
            if index == 0 {
                continue;
            }
            if active_map() != Some(entity.map) {
                return None;
            }
            let reading = StringRead::begin()?;
            // SAFETY: The bit pattern comes from the engine's own string_t.
            // GoldSrc uses signed offsets at this callback boundary.
            let string = unsafe { lookup(index as i32) };
            // SAFETY: The edict slot remains readable while its map is active.
            if !reading.uninterrupted() || !unsafe { entity.still_matches(edict) } {
                return None;
            }
            // SAFETY: The current-map engine string can be read until the
            // next native call. Copy it before revalidating the edict.
            *value = unsafe { copy_string(string, MAX_ENGINE_STRING) };
            drop(reading);
            entity.resolve(engine)?;
        }
        let [
            classname,
            globalname,
            target,
            targetname,
            model,
            netname,
            message,
        ] = strings;
        Some(EntityInfo {
            entity,
            kind,
            keyvalues: EntityKeyValues {
                classname,
                globalname,
                target,
                targetname,
                model,
                netname,
                message,
            },
        })
    }

    /// Copy current spatial fields after validating the allocation generation.
    /// Bounds are the engine's world-space absolute bounds. A stale or
    /// unresolved handle returns `None`.
    pub fn entity_spatial(&self, entity: EntityHandle) -> Option<EntitySpatial> {
        let edict = entity.resolve(__ffi::engine()?)?;
        // SAFETY: The handle resolved to a live edict. Copy initialized
        // vector fields without creating references or calling the engine.
        unsafe {
            let vars = ptr::addr_of!((*edict).v);
            Some(EntitySpatial {
                entity,
                origin: vector(ptr::addr_of!((*vars).origin).read()),
                angles: vector(ptr::addr_of!((*vars).angles).read()),
                velocity: vector(ptr::addr_of!((*vars).velocity).read()),
                bounds_min: vector(ptr::addr_of!((*vars).absmin).read()),
                bounds_max: vector(ptr::addr_of!((*vars).absmax).read()),
            })
        }
    }
}

type EdictLookup = unsafe extern "C" fn(i32) -> *mut edict_t;

fn discovery() -> Result<(u64, i32, EdictLookup), EntityError> {
    let map = active_map().ok_or(EntityError::Unavailable)?;
    let limit = LIMIT.get();
    let lookup = __ffi::engine()
        .and_then(|engine| engine.entity)
        .ok_or(EntityError::Unavailable)?;
    if limit <= 0 {
        return Err(EntityError::Unavailable);
    }
    Ok((map, limit, lookup))
}

/// Whether the engine includes monsters in collision checks.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TraceMonsters {
    #[default]
    /// Include monsters in native collision checks (the default).
    Include,
    /// Request that the native trace ignore monsters.
    Ignore,
}

/// The four standard GoldSrc hull numbers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum Hull {
    /// Point-sized hull (native hull 0).
    Point = raw::point_hull as i32,
    /// Standard standing-player hull (native hull 1).
    Human = raw::human_hull as i32,
    /// Standard large hull (native hull 2); dimensions belong to the host.
    Large = raw::large_hull as i32,
    /// Standard head/crouching hull (native hull 3); dimensions belong to the host.
    Head = raw::head_hull as i32,
}

/// Collision filtering shared by line and hull traces.
/// The default includes monsters and skips no entity.
#[derive(Clone, Copy, Debug, Default)]
pub struct TraceOptions {
    /// Whether the native trace includes monsters.
    pub monsters: TraceMonsters,
    /// Optional entity to exclude. A stale handle causes
    /// [`TraceError::StaleEntity`] rather than tracing against a replacement.
    pub skip: Option<EntityHandle>,
}

/// Engine-reported collision plane in world coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TracePlane {
    /// Native plane normal; copied without normalization by the SDK.
    pub normal: [f32; 3],
    /// Native plane distance from the world origin in engine units.
    pub distance: f32,
}

/// Owned trace output. An absent hit handle can also mean the native hit
/// became invalid during resolution. Use the fraction to detect a collision.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TraceResult {
    /// The engine reports that the entire trace is inside solid space.
    pub all_solid: bool,
    /// The engine reports that the starting position is inside solid space.
    pub start_solid: bool,
    /// The engine reports that the trace passes through open space.
    pub in_open: bool,
    /// The engine reports that the trace passes through water.
    pub in_water: bool,
    /// Native fraction of the segment completed before collision; conventionally
    /// `1.0` means unobstructed. The SDK does not clamp the host value.
    pub fraction: f32,
    /// Native trace endpoint in world-space engine units.
    pub end_position: [f32; 3],
    /// Collision plane when the trace is not all-solid and its fraction is below
    /// `1.0`; otherwise `None`.
    pub plane: Option<TracePlane>,
    /// Resolved identity of the hit entity, if still valid during resolution.
    /// `None` alone does not prove that the trace was unobstructed.
    pub hit: Option<EntityHandle>,
    /// Native hit-group number; its interpretation is GameDLL-specific.
    pub hit_group: i32,
}

/// A trace could not complete with the required lifecycle and input checks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TraceError {
    /// No active map, engine access, or required native trace callback is available.
    Unavailable,
    /// At least one endpoint component is NaN or infinite.
    InvalidCoordinates,
    /// The entity requested in [`TraceOptions::skip`] no longer resolves.
    StaleEntity,
    /// Map retirement or shutdown was observed during the trace or hit resolution.
    MapChanged,
}

impl fmt::Display for TraceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unavailable => "tracing requires an active map and engine callbacks",
            Self::InvalidCoordinates => "trace coordinates must be finite",
            Self::StaleEntity => "the entity to skip is stale",
            Self::MapChanged => "the map changed during the trace",
        })
    }
}

impl std::error::Error for TraceError {}

/// Finite world-space endpoints shared by line and hull traces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TraceSegment {
    start: [f32; 3],
    end: [f32; 3],
}

impl TraceSegment {
    /// Validate both endpoints. NaN and infinities return
    /// `TraceError::InvalidCoordinates`.
    ///
    /// A constant can require finite endpoints at compile time:
    ///
    /// ```compile_fail,E0080
    /// use metamod_sdk_rs::TraceSegment;
    ///
    /// const SEGMENT: TraceSegment = match TraceSegment::new([0.0; 3], [f32::INFINITY, 0.0, 0.0]) {
    ///     Ok(segment) => segment,
    ///     Err(_) => panic!("trace endpoints must be finite"),
    /// };
    /// ```
    pub const fn new(start: [f32; 3], end: [f32; 3]) -> Result<Self, TraceError> {
        let mut axis = 0;
        while axis < start.len() {
            if !start[axis].is_finite() || !end[axis].is_finite() {
                return Err(TraceError::InvalidCoordinates);
            }
            axis += 1;
        }
        Ok(Self { start, end })
    }

    /// Return the finite world-space starting point in engine units.
    pub const fn start(self) -> [f32; 3] {
        self.start
    }

    /// Return the finite world-space ending point in engine units.
    pub const fn end(self) -> [f32; 3] {
        self.end
    }
}

impl Server<'_, '_> {
    /// Trace a point along a finite segment and copy the native collision result.
    ///
    /// Returns an error if engine access is unavailable, the skip identity is
    /// stale, or the map changes during the operation. A returned hit handle
    /// can still expire after this call; see [`TraceResult::hit`].
    ///
    /// ```
    /// use metamod_sdk_rs::{Context, EntityHandle, TraceError, TraceOptions, TraceSegment};
    ///
    /// fn probe(ctx: &Context<'_>, skip: EntityHandle) -> Result<(), TraceError> {
    ///     let segment = TraceSegment::new([0.0, 0.0, 32.0], [128.0, 0.0, 32.0])?;
    ///     let result = ctx.server().trace_line(segment, TraceOptions {
    ///         skip: Some(skip),
    ///         ..TraceOptions::default()
    ///     })?;
    ///     if result.fraction < 1.0 {
    ///         // A collision need not have a resolvable hit identity.
    ///         if let Some(hit) = result.hit {
    ///             let _still_current = ctx.is_entity_current(hit);
    ///         }
    ///     }
    ///     Ok(())
    /// }
    /// ```
    pub fn trace_line(
        &self,
        segment: TraceSegment,
        options: TraceOptions,
    ) -> Result<TraceResult, TraceError> {
        trace(segment, options, None)
    }

    /// Sweep a standard native hull along a finite segment.
    ///
    /// Uses the same filtering and lifecycle checks as [`Self::trace_line`].
    /// Hull dimensions and collision behavior are supplied by the host.
    pub fn trace_hull(
        &self,
        segment: TraceSegment,
        hull: Hull,
        options: TraceOptions,
    ) -> Result<TraceResult, TraceError> {
        trace(segment, options, Some(hull))
    }
}

fn trace(
    segment: TraceSegment,
    options: TraceOptions,
    hull: Option<Hull>,
) -> Result<TraceResult, TraceError> {
    let engine = __ffi::engine().ok_or(TraceError::Unavailable)?;
    let map = active_map().ok_or(TraceError::Unavailable)?;
    let skip = match options.skip {
        Some(handle) => handle.resolve(engine).ok_or(TraceError::StaleEntity)?,
        None => ptr::null_mut(),
    };
    let monsters = match options.monsters {
        TraceMonsters::Include => raw::dont_ignore_monsters as i32,
        TraceMonsters::Ignore => raw::ignore_monsters as i32,
    };
    let mut output = raw::TraceResult {
        flFraction: 1.0,
        ..Default::default()
    };
    // SAFETY: The vectors and output live through the synchronous call. The
    // skip edict, if present, was just resolved with no intervening native call.
    unsafe {
        match hull {
            Some(hull) => engine.trace_hull.ok_or(TraceError::Unavailable)?(
                segment.start.as_ptr(),
                segment.end.as_ptr(),
                monsters,
                hull as i32,
                skip,
                &mut output,
            ),
            None => engine.trace_line.ok_or(TraceError::Unavailable)?(
                segment.start.as_ptr(),
                segment.end.as_ptr(),
                monsters,
                skip,
                &mut output,
            ),
        }
    }
    if active_map() != Some(map) {
        return Err(TraceError::MapChanged);
    }
    // SAFETY: The host trace returned null or a readable hit edict for
    // this still-active map. No native call has intervened.
    let hit = unsafe { capture_hit(engine, map, output.pHit) };
    if active_map() != Some(map) {
        return Err(TraceError::MapChanged);
    }
    Ok(TraceResult {
        all_solid: output.fAllSolid != 0,
        start_solid: output.fStartSolid != 0,
        in_open: output.fInOpen != 0,
        in_water: output.fInWater != 0,
        fraction: output.flFraction,
        end_position: vector(output.vecEndPos),
        plane: (output.fAllSolid == 0 && output.flFraction < 1.0).then_some(TracePlane {
            normal: vector(output.vecPlaneNormal),
            distance: output.flPlaneDist,
        }),
        hit,
        hit_group: output.iHitgroup,
    })
}

/// # Safety
/// The trace result's edict must be null or live in the active map arena.
unsafe fn capture_hit(engine: Engine, map: u64, edict: *mut edict_t) -> Option<EntityHandle> {
    let index_of = engine.index?;
    if LIMIT.get() == 0 {
        return None;
    }
    // SAFETY: The trace returned a hit edict in the still-active map or null.
    let serial = unsafe { live_serial(edict) }?;
    // SAFETY: No native call intervened since the trace returned this edict.
    let index = unsafe { index_of(edict) };
    if active_map() != Some(map) || !valid_index(index) {
        return None;
    }
    let handle = EntityHandle { index, serial, map };
    (handle.resolve(engine) == Some(edict)).then_some(handle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_reject_freed_reused_and_previous_map_edicts() {
        thread_local! {
            static EDICT: Cell<*mut edict_t> = const { Cell::new(ptr::null_mut()) };
        }
        unsafe extern "C" fn entity(index: i32) -> *mut edict_t {
            if index == 7 {
                EDICT.get()
            } else {
                ptr::null_mut()
            }
        }
        let mut edict = edict_t {
            serialnumber: 42,
            ..Default::default()
        };
        let edict = &raw mut edict;
        EDICT.set(edict);
        let engine = Engine {
            entity: Some(entity),
            ..Default::default()
        };
        let mut globals = EngineGlobals {
            maxEntities: 8,
            ..Default::default()
        };
        // SAFETY: The fixture is live until the test clears the pointer.
        unsafe { set_globals(&raw mut globals) };
        activate_map();
        let handle = EntityHandle {
            index: 7,
            serial: 42,
            map: active_map().unwrap(),
        };
        assert!(handle.resolve(engine).is_some());
        // SAFETY: The fixture edict lives through the entire test.
        unsafe { (*edict).free = 1 };
        assert!(handle.resolve(engine).is_none());
        unsafe {
            (*edict).free = 0;
            (*edict).serialnumber = 43;
        }
        assert!(handle.resolve(engine).is_none());
        unsafe { (*edict).serialnumber = 42 };
        invalidate_map();
        assert!(handle.resolve(engine).is_none());
        activate_map();
        assert!(handle.resolve(engine).is_none());
        EDICT.set(ptr::null_mut());
        unsafe { set_globals(ptr::null_mut()) };
    }

    #[test]
    fn discovery_discards_retired_arena_pointer_after_lookup() {
        let mut globals = EngineGlobals {
            maxEntities: 8,
            ..Default::default()
        };
        let old_edict = Box::into_raw(Box::new(edict_t::default()));
        // SAFETY: The test globals remain allocated until cleared below.
        unsafe { set_globals(&raw mut globals) };
        activate_map();
        let map = active_map().unwrap();
        // SAFETY: The lookup retires the map before deallocating the slot.
        // The old pointer is an opaque return value on this path.
        let result = unsafe {
            find_entity(
                |_| {
                    invalidate_map();
                    drop(Box::from_raw(old_edict));
                    activate_map();
                    old_edict
                },
                7,
                map,
            )
        };
        assert_eq!(result, Err(EntityError::MapChanged));
        unsafe { set_globals(ptr::null_mut()) };
        invalidate_map();
    }

    #[test]
    fn resolve_discards_a_pointer_returned_from_a_retired_map() {
        unsafe extern "C" fn lookup(_: i32) -> *mut edict_t {
            activate_map();
            // Model a native wrapper returning an old arena's address.
            ptr::dangling_mut()
        }

        let mut globals = EngineGlobals {
            maxEntities: 8,
            ..Default::default()
        };
        // SAFETY: The fixture stays allocated until its pointer is cleared.
        unsafe { set_globals(&raw mut globals) };
        activate_map();
        let handle = EntityHandle {
            index: 7,
            serial: 42,
            map: active_map().unwrap(),
        };
        let engine = Engine {
            entity: Some(lookup),
            ..Default::default()
        };
        assert!(handle.resolve(engine).is_none());
        invalidate_map();
        unsafe { set_globals(ptr::null_mut()) };
    }

    #[test]
    fn invalid_capacities_revoke_the_index_range_without_native_lookup() {
        let mut globals = EngineGlobals::default();
        let globals_ptr = &raw mut globals;
        // SAFETY: The fixture stays alive until the pointer is cleared.
        unsafe { set_globals(globals_ptr) };
        for capacity in [0, -1, MAX_EDICT_CAPACITY + 1, i32::MAX] {
            // SAFETY: The fixture is owned by this test thread.
            unsafe { (*globals_ptr).maxEntities = 8 };
            activate_map();
            assert!(valid_index(7));
            assert!(!valid_index(8));
            unsafe { (*globals_ptr).maxEntities = capacity };
            activate_map();
            assert!(!valid_index(0));
            assert!(matches!(discovery(), Err(EntityError::Unavailable)));
            let map = active_map().unwrap();
            assert_eq!(
                // SAFETY: The fixture lookup never returns any pointer.
                unsafe { find_entity(|_| panic!("invalid index reached FFI"), 0, map) },
                Ok(None)
            );
        }
        unsafe { set_globals(ptr::null_mut()) };
        activate_map();
        assert!(!valid_index(0));
    }

    #[test]
    fn player_entity_handles_require_a_valid_client_capacity() {
        let mut globals = EngineGlobals {
            maxEntities: 8,
            ..Default::default()
        };
        let globals = &raw mut globals;
        // SAFETY: This fixture stays allocated and owner-thread confined.
        unsafe {
            set_globals(globals);
            for max_clients in [i32::MIN, 0, 8, i32::MAX] {
                (*globals).maxClients = max_clients;
                activate_map();
                assert!(tracked_entity(1, 0).is_none());
            }
            (*globals).maxClients = 7;
            activate_map();
            assert!(tracked_entity(7, 42).is_some_and(|handle| handle.serial == 42));
            assert!(tracked_entity(8, 42).is_none());
            set_globals(ptr::null_mut());
        }
        invalidate_map();
    }

    #[test]
    fn trace_segment_validates_both_endpoints_in_const_and_runtime_contexts() {
        const SEGMENT: TraceSegment = match TraceSegment::new([0.0; 3], [1.0, -2.0, 3.0]) {
            Ok(segment) => segment,
            Err(_) => panic!("fixed coordinates are finite"),
        };
        const INVALID: Result<TraceSegment, TraceError> =
            TraceSegment::new([0.0; 3], [0.0, f32::INFINITY, 0.0]);
        assert_eq!(SEGMENT.start(), [0.0; 3]);
        assert_eq!(SEGMENT.end(), [1.0, -2.0, 3.0]);
        assert_eq!(INVALID, Err(TraceError::InvalidCoordinates));
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            for axis in 0..3 {
                let mut invalid = [0.0; 3];
                invalid[axis] = value;
                let expected = Err(TraceError::InvalidCoordinates);
                assert_eq!(TraceSegment::new(invalid, [0.0; 3]), expected);
                assert_eq!(TraceSegment::new([0.0; 3], invalid), expected);
            }
        }
    }
}
