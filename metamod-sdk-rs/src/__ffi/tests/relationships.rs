#[test]
fn entity_relationships_observe_slots_without_reading_foreign_targets() {
    thread_local! {
        static SOURCE: Cell<*mut edict_t> = const { Cell::new(ptr::null_mut()) };
        static TARGET: Cell<*mut edict_t> = const { Cell::new(ptr::null_mut()) };
        static MODE: Cell<u8> = const { Cell::new(0) };
    }
    unsafe extern "C" fn lookup(index: c_int) -> *mut edict_t {
        if index == 1 {
            match MODE.replace(0) {
                1 => {
                    crate::trace::invalidate_map();
                    // SAFETY: The fixture owns this source allocation. Retire
                    // its map before freeing it; the interrupted scan must not
                    // touch the old source when the native lookup returns.
                    unsafe { drop(Box::from_raw(SOURCE.replace(ptr::null_mut()))) };
                    return ptr::dangling_mut();
                }
                // SAFETY: Mock slots remain allocated through every callback.
                2 => unsafe { (*SOURCE.get()).serialnumber += 1 },
                3 => unsafe { (*SOURCE.get()).v.owner = ptr::null_mut() },
                4 => unsafe { (*TARGET.get()).serialnumber += 1 },
                5 => SHUTDOWN.set(true),
                _ => {}
            }
        }
        match index {
            0 => SOURCE.get(),
            2 => TARGET.get(),
            _ => ptr::null_mut(),
        }
    }
    #[derive(Default)]
    struct Fixture;
    impl Plugin for Fixture {
        const METADATA: Metadata = METADATA;
    }
    let mut target = edict_t {
        serialnumber: 7,
        ..Default::default()
    };
    SOURCE.set(Box::into_raw(Box::new(edict_t::default())));
    TARGET.set(&raw mut target);
    // SAFETY: All accesses to these native mock records use the stored raw
    // pointers. The source stays allocated until the explicit map retirement.
    unsafe {
        (*SOURCE.get()).v.owner = TARGET.get();
        (*SOURCE.get()).v.enemy = ptr::dangling_mut();
        (*SOURCE.get()).v.aiment = SOURCE.get();
    }
    ENGINE.set(Engine {
        entity: Some(lookup),
        ..Default::default()
    });
    let mut globals = EngineGlobals {
        maxEntities: 4,
        ..Default::default()
    };
    // SAFETY: Initialized globals outlive the active map and callbacks.
    unsafe { crate::trace::set_globals(&raw mut globals) };
    crate::trace::activate_map();
    let scope = ();
    let ctx = make_context::<Fixture>(
        &scope,
        Services {
            id: ptr::null_mut(),
            log: None,
            console: None,
            user_message: None,
        },
    );
    let source_id = ctx.entity_at(0).unwrap().unwrap();
    let first = ctx.entity_relationships(source_id).unwrap();
    assert_eq!(first.owner.unwrap().index(), 2);
    assert_eq!(first.aiment, Some(source_id));
    assert_eq!(first.enemy, None);
    assert_eq!(first.ground_entity, None);
    let retired = Box::into_raw(Box::new(edict_t::default()));
    // SAFETY: The fixture owns this allocation; only the opaque address is
    // retained in the source. A relationship must not make it readable again.
    unsafe {
        (*SOURCE.get()).v.enemy = retired;
        drop(Box::from_raw(retired));
    }
    assert_eq!(ctx.entity_relationships(source_id).unwrap().enemy, None);
    // A raw relationship cannot retain the original occupant's serial.
    MODE.set(4);
    let reused = ctx.entity_relationships(source_id).unwrap().owner.unwrap();
    assert_ne!(Some(reused), first.owner);
    assert!(!ctx.is_entity_current(first.owner.unwrap()));
    // SAFETY: Target remains allocated, even while marked vacant.
    unsafe { (*TARGET.get()).free = 1 };
    assert_eq!(ctx.entity_relationships(source_id).unwrap().owner, None);
    unsafe { (*TARGET.get()).free = 0 };
    MODE.set(3);
    assert!(ctx.entity_relationships(source_id).is_none());
    unsafe { (*SOURCE.get()).v.owner = TARGET.get() };
    MODE.set(2);
    assert!(ctx.entity_relationships(source_id).is_none());
    let source_id = ctx.entity_at(0).unwrap().unwrap();
    MODE.set(1);
    assert!(ctx.entity_relationships(source_id).is_none());
    crate::trace::activate_map();
    assert!(ctx.entity_relationships(source_id).is_none());
    SOURCE.set(Box::into_raw(Box::new(edict_t::default())));
    // SAFETY: Initialize the replacement source using its owning raw pointer.
    unsafe { (*SOURCE.get()).v.owner = TARGET.get() };
    let source_id = ctx.entity_at(0).unwrap().unwrap();
    MODE.set(5);
    assert!(ctx.entity_relationships(source_id).is_none());
    SHUTDOWN.set(false);
    crate::trace::invalidate_map();
    // SAFETY: No further callbacks use the replacement source or these globals.
    unsafe {
        drop(Box::from_raw(SOURCE.replace(ptr::null_mut())));
        crate::trace::set_globals(ptr::null_mut());
    }
}
