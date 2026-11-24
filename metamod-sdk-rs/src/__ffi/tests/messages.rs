#[test]
fn message_send_preserves_bytes_and_rejects_nested_and_interrupted_sends() {
    use crate::{MessageSendError, UserMessagePayload};
    thread_local! {
        static EDICT: Cell<*mut edict_t> = const { Cell::new(ptr::null_mut()) };
        static CALLS: RefCell<Vec<i32>> = const { RefCell::new(Vec::new()) };
        static ON_BEGIN: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
    }
    unsafe extern "C" fn lookup(
        _: *mut PluginInfo,
        _: *const c_char,
        size: *mut c_int,
    ) -> c_int {
        // SAFETY: SDK provides writable initialized size storage.
        unsafe { size.write(2) };
        64
    }
    unsafe extern "C" fn entity(_: c_int) -> *mut edict_t {
        EDICT.get()
    }
    unsafe extern "C" fn begin(dest: c_int, id: c_int, _: *const f32, _: *mut edict_t) {
        CALLS.with(|calls| calls.borrow_mut().extend([dest, id]));
        let callback = ON_BEGIN.with(|slot| slot.borrow_mut().take());
        if let Some(callback) = callback {
            callback();
        }
    }
    unsafe extern "C" fn write(value: c_int) {
        CALLS.with(|calls| calls.borrow_mut().push(value));
    }
    unsafe extern "C" fn end() {
        CALLS.with(|calls| calls.borrow_mut().push(-1));
    }
    #[derive(Default)]
    struct Fixture;
    impl Plugin for Fixture {
        const METADATA: Metadata = METADATA;
    }
    let mut edict = edict_t::default();
    EDICT.set(&raw mut edict);
    let mut globals = EngineGlobals {
        maxClients: 2,
        maxEntities: 8,
        ..Default::default()
    };
    // SAFETY: Mock native storage remains live through every callback below.
    unsafe { crate::trace::set_globals(&raw mut globals) };
    ENGINE.set(Engine {
        entity: Some(entity),
        message_begin: Some(begin),
        write_byte: Some(write),
        message_end: Some(end),
        ..Default::default()
    });
    let services = Services {
        id: ptr::null_mut(),
        log: None,
        console: None,
        user_message: Some(lookup),
    };
    let scope = ();
    let ctx = make_context::<Fixture>(&scope, services);
    crate::trace::activate_map();
    const SLOT: PlayerSlot = PlayerSlot::new(1).unwrap();
    let player = EVENTS
        .with(|events| events.borrow_mut().join(SLOT, 0))
        .unwrap();
    let mut payload = UserMessagePayload::new();
    payload.write_short(-2).unwrap();
    // SAFETY: Controlled callbacks preserve storage and native ownership.
    // The mock records bytes rather than passing them to a client parser.
    let sender = unsafe { ctx.message_sender() };
    assert_eq!(sender.send_to(c"Fixed", &payload, player), Ok(()));
    assert_eq!(
        CALLS.with(|calls| calls.borrow().clone()),
        [1, 64, 254, 255, -1]
    );
    CALLS.with(|calls| calls.borrow_mut().clear());
    assert!(matches!(
        sender.send_to(c"Fixed", &UserMessagePayload::new(), player),
        Err(MessageSendError::Payload(_))
    ));
    assert!(CALLS.with(|calls| calls.borrow().is_empty()));
    let nested_result = std::rc::Rc::new(Cell::new(None));
    let result = nested_result.clone();
    let nested_payload = payload.clone();
    ON_BEGIN.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move || {
            // SAFETY: Same controlled mock; the nested send must stop before native begin.
            let nested_scope = ();
            let nested_ctx = make_context::<Fixture>(&nested_scope, services);
            let nested = unsafe { nested_ctx.message_sender() };
            result.set(Some(nested.send_to(c"Fixed", &nested_payload, player)));
        }))
    });
    assert_eq!(sender.send_to(c"Fixed", &payload, player), Ok(()));
    assert_eq!(nested_result.get(), Some(Err(MessageSendError::Busy)));
    CALLS.with(|calls| calls.borrow_mut().clear());
    let engine = ENGINE.get();
    ENGINE.set(Engine {
        message_end: None,
        ..engine
    });
    assert_eq!(
        sender.send_to(c"Fixed", &payload, player),
        Err(MessageSendError::Unavailable)
    );
    assert!(CALLS.with(|calls| calls.borrow().is_empty()));
    ENGINE.set(engine);
    // Deliberately violate non-reentry with valid mock memory. The SDK
    // must stop subsequent native calls, without an automatic MessageEnd.
    ON_BEGIN.with(|slot| *slot.borrow_mut() = Some(Box::new(crate::trace::invalidate_map)));
    assert_eq!(
        sender.send_to(c"Fixed", &payload, player),
        Err(MessageSendError::Interrupted)
    );
    assert_eq!(CALLS.with(|calls| calls.borrow().clone()), [1, 64]);
    crate::trace::activate_map();
    ON_BEGIN.with(|slot| *slot.borrow_mut() = Some(Box::new(|| SHUTDOWN.set(true))));
    CALLS.with(|calls| calls.borrow_mut().clear());
    assert_eq!(
        sender.send_to(c"Fixed", &payload, player),
        Err(MessageSendError::Interrupted)
    );
    assert_eq!(CALLS.with(|calls| calls.borrow().clone()), [1, 64]);
    SHUTDOWN.set(false);
    reset_players();
    CALLS.with(|calls| calls.borrow_mut().clear());
    assert_eq!(
        sender.send_to(c"Fixed", &payload, player),
        Err(MessageSendError::StaleRecipient)
    );
    assert!(CALLS.with(|calls| calls.borrow().is_empty()));
    crate::trace::invalidate_map();
    EDICT.set(ptr::null_mut());
    // SAFETY: Clear the mock pointer before its allocation leaves scope.
    unsafe { crate::trace::set_globals(ptr::null_mut()) };
}

#[test]
fn message_lookup_and_recipient_reject_results_invalidated_by_native_reentry() {
    use crate::{MessageLookupError, StaleMessageRecipient, UserMessageSize};

    thread_local! {
        static EDICT: Cell<*mut edict_t> = const { Cell::new(ptr::null_mut()) };
        static REUSE: Cell<bool> = const { Cell::new(false) };
    }
    const SLOT: PlayerSlot = PlayerSlot::new(1).unwrap();

    unsafe extern "C" fn lookup(
        _: *mut PluginInfo,
        name: *const c_char,
        size: *mut c_int,
    ) -> c_int {
        // SAFETY: The test supplies the name and initialized writable size slot.
        match unsafe { CStr::from_ptr(name) }.to_bytes() {
            b"Fixed" => {
                unsafe { size.write(2) };
                64
            }
            b"Variable" => {
                unsafe { size.write(-1) };
                255
            }
            b"MissingSize" => 65,
            b"Transition" => {
                crate::trace::activate_map();
                unsafe { size.write(2) };
                64
            }
            _ => 0,
        }
    }

    unsafe extern "C" fn entity(_: c_int) -> *mut edict_t {
        if REUSE.replace(false) {
            // Simulate same-slot reuse with the same native serial.
            EVENTS.with(|events| events.borrow_mut().join(SLOT, 0));
        }
        EDICT.get()
    }

    #[derive(Default)]
    struct Fixture;
    impl Plugin for Fixture {
        const METADATA: Metadata = METADATA;
    }

    let mut edict = edict_t::default();
    EDICT.set(&raw mut edict);
    let mut globals = EngineGlobals {
        maxClients: 2,
        maxEntities: 8,
        ..Default::default()
    };
    // SAFETY: The fixture retains its engine globals for every map below.
    unsafe { crate::trace::set_globals(&raw mut globals) };
    ENGINE.set(Engine {
        entity: Some(entity),
        ..Default::default()
    });
    let services = Services {
        id: ptr::null_mut(),
        log: None,
        console: None,
        user_message: Some(lookup),
    };
    let scope = ();
    let ctx = make_context::<Fixture>(&scope, services);
    let mm = ctx.metamod();
    assert_eq!(
        mm.user_message(c"Fixed"),
        Err(MessageLookupError::Unavailable)
    );
    crate::trace::activate_map();
    let without_lookup = make_context::<Fixture>(
        &scope,
        Services {
            user_message: None,
            ..services
        },
    );
    assert_eq!(
        without_lookup.metamod().user_message(c"Fixed"),
        Err(MessageLookupError::Unavailable)
    );
    let fixed = mm.user_message(c"Fixed").unwrap();
    assert_eq!((fixed.id(), fixed.size()), (64, UserMessageSize::Fixed(2)));
    assert_eq!(
        mm.user_message(c"Variable").unwrap().size(),
        UserMessageSize::Variable
    );
    assert_eq!(
        mm.user_message(c"MissingSize"),
        Err(MessageLookupError::InvalidRegistration)
    );
    assert_eq!(
        mm.user_message(c"Unknown"),
        Err(MessageLookupError::NotRegistered)
    );
    let player = EVENTS
        .with(|events| events.borrow_mut().join(SLOT, 0))
        .unwrap();
    assert_eq!(ctx.message_recipient(player).unwrap().player(), player);
    REUSE.set(true);
    assert_eq!(ctx.message_recipient(player), Err(StaleMessageRecipient));
    let replacement = player_at(SLOT).unwrap();
    assert_ne!(replacement, player);
    assert_eq!(
        ctx.message_recipient(replacement).unwrap().player(),
        replacement
    );
    assert_eq!(
        mm.user_message(c"Transition"),
        Err(MessageLookupError::Unavailable)
    );
    crate::trace::invalidate_map();
    reset_players();
    assert_eq!(
        ctx.message_recipient(replacement),
        Err(StaleMessageRecipient)
    );
}

