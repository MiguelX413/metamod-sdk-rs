#[test]
fn disconnect_lookup_rejects_a_replaced_edict() {
    unsafe extern "C" fn replace(edict: *const edict_t) -> c_int {
        // SAFETY: The test passes a writable, live edict slot.
        unsafe { (*edict.cast_mut()).serialnumber = 2 };
        1
    }

    #[derive(Default)]
    struct TestPlugin;
    impl Plugin for TestPlugin {
        const METADATA: Metadata = METADATA;
    }

    queried();
    ENGINE.set(Engine {
        index: Some(replace),
        ..Default::default()
    });
    let mut globals = MetaGlobals::default();
    let mut functions = MetaFunctions::default();
    let mut edict = edict_t {
        serialnumber: 1,
        ..Default::default()
    };
    let slot = PlayerSlot::new(1).unwrap();
    // SAFETY: The tables and edict stay live for all hook calls.
    unsafe {
        assert_eq!(
            attach::<TestPlugin>(PT_STARTUP, &mut functions, &raw mut globals),
            1
        );
        let player = EVENTS
            .with(|events| events.borrow_mut().join(slot, 2))
            .unwrap();
        player_disconnect::<TestPlugin>(&raw mut edict);
        assert_eq!(player_at(slot), Some(player));
        player_disconnect::<TestPlugin>(&raw mut edict);
        assert_eq!(player_at(slot), None);
        game_shutdown::<TestPlugin>();
    }
}

#[test]
fn public_player_slot_constructor_checks_the_engine_index_range() {
    assert_eq!(PlayerSlot::new(0), None);
    const SLOT: PlayerSlot = PlayerSlot::new(7).unwrap();
    const INDEX: i32 = SLOT.get();
    assert_eq!(INDEX, 7);
    assert_eq!(SLOT.to_string(), "7");
    assert_eq!(PlayerSlot::new(INDEX), Some(SLOT));
    assert_eq!(const { PlayerSlot::new(i32::MAX).unwrap() }.get(), i32::MAX);
    assert_eq!(PlayerSlot::new(-1), None);
    assert_eq!(PlayerSlot::new(i32::MIN), None);
}

#[test]
fn player_slot_traits_round_trip_and_reject_invalid_indices() {
    for index in [1, 7, i32::MAX] {
        let slot = PlayerSlot::try_from(index).unwrap();
        assert_eq!(Some(slot), PlayerSlot::new(index));
        assert_eq!(i32::from(slot), index);
    }
    for index in [0, -1, i32::MIN] {
        assert!(PlayerSlot::try_from(index).is_err());
    }
}

#[test]
fn session_ids_expire_on_disconnect_slot_reuse_and_map_boundaries() {
    let slot = const { PlayerSlot::new(7).unwrap() };
    let first = EVENTS
        .with(|events| events.borrow_mut().join(slot, 0))
        .unwrap();
    assert_eq!(player_at(slot), Some(first));
    let ended = EVENTS.with(|events| events.borrow_mut().players.remove(&slot));
    assert_eq!(ended, Some((first, 0)));
    assert_eq!(player_at(slot), None);
    let second = EVENTS
        .with(|events| events.borrow_mut().join(slot, 0))
        .unwrap();
    assert_ne!(first, second);
    reset_players();
    assert_eq!(player_at(slot), None);
    let third = EVENTS
        .with(|events| events.borrow_mut().join(slot, 0))
        .unwrap();
    assert_ne!(second, third);
    let replacement = EVENTS
        .with(|events| events.borrow_mut().join(slot, 0))
        .unwrap();
    assert_ne!(third, replacement);
    assert_eq!(player_at(slot), Some(replacement));
}

