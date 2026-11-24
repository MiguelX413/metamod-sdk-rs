#[test]
fn server_state_copies_globals_and_rejects_interrupted_strings() {
    thread_local! {
        static BUFFER: Cell<*mut u8> = const { Cell::new(ptr::null_mut()) };
        static MODE: Cell<u8> = const { Cell::new(0) };
        static NESTED_REJECTED: Cell<bool> = const { Cell::new(false) };
    }
    #[derive(Default)]
    struct Fixture;
    impl Plugin for Fixture {
        const METADATA: Metadata = METADATA;
    }
    fn context(scope: &()) -> Context<'_> {
        make_context::<Fixture>(
            scope,
            Services {
                id: ptr::null_mut(),
                log: None,
                console: None,
                user_message: None,
            },
        )
    }
    unsafe extern "C" fn lookup(index: c_int) -> *const c_char {
        match MODE.get() {
            1 => {
                crate::trace::invalidate_map();
                return ptr::dangling();
            }
            2 => {
                let scope = ();
                NESTED_REJECTED.set(context(&scope).server().state().is_none());
                return ptr::dangling();
            }
            3 => return ptr::null(),
            8 => return c"".as_ptr(),
            9 => return c"\xff".as_ptr(),
            4 => {
                // SAFETY: The mock owns the full 4,096-byte initialized buffer.
                unsafe { BUFFER.get().write_bytes(b'x', 4096) };
                return BUFFER.get().cast();
            }
            5 => {
                // SAFETY: Both operations address the mock's live buffer.
                unsafe {
                    BUFFER.get().write_bytes(b'x', 4095);
                    BUFFER.get().add(4095).write(0);
                }
                return BUFFER.get().cast();
            }
            6 if index == 2 => {
                crate::trace::activate_map();
                return ptr::dangling();
            }
            7 => {
                SHUTDOWN.set(true);
                return ptr::dangling();
            }
            _ => {}
        }
        let bytes = if index == 1 {
            c"arena".to_bytes_with_nul()
        } else {
            c"spot".to_bytes_with_nul()
        };
        // SAFETY: Fixed input and destination ranges are live and disjoint.
        unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), BUFFER.get(), bytes.len()) };
        BUFFER.get().cast()
    }

    let mut globals = EngineGlobals {
        time: 12.5,
        frametime: 0.02,
        maxClients: 7,
        maxEntities: 8,
        deathmatch: 2.5,
        coop: -1.0,
        teamplay: 3.0,
        serverflags: 16.0,
        mapname: 1,
        startspot: 2,
        ..Default::default()
    };
    let globals = &raw mut globals;
    let mut buffer = [0; 4096];
    BUFFER.set(buffer.as_mut_ptr());
    ENGINE.set(Engine {
        string_from_index: Some(lookup),
        ..Default::default()
    });
    let scope = ();
    let ctx = context(&scope);
    // SAFETY: The test retains initialized globals through all native calls.
    unsafe { crate::trace::set_globals(globals) };
    assert!(ctx.server().state().is_none());
    crate::trace::activate_map();
    let first = ctx.server().state().unwrap();
    assert_eq!((first.time, first.frame_time), (Some(12.5), Some(0.02)));
    assert_eq!((first.max_clients, first.max_entities), (Some(7), Some(8)));
    assert_eq!(
        (
            first.deathmatch,
            first.coop,
            first.teamplay,
            first.server_flags
        ),
        (Some(2.5), Some(-1.0), Some(3.0), Some(16.0))
    );
    assert_eq!(first.map_name.as_deref(), Some(c"arena"));
    assert_eq!(first.start_spot.as_deref(), Some(c"spot"));
    // The second getter reused the first buffer. The first result remains owned.
    // SAFETY: Read from the same live mock buffer pointer used by native writes.
    assert_eq!(
        unsafe { std::slice::from_raw_parts(BUFFER.get(), 5) },
        b"spot\0"
    );
    // Null, unterminated, and maximum-length getter results.
    for mode in [3, 4, 5] {
        MODE.set(mode);
        let state = ctx.server().state().unwrap();
        let expected = if mode == 5 { Some(4095) } else { None };
        assert_eq!(state.map_name.map(|name| name.to_bytes().len()), expected);
    }
    MODE.set(8);
    assert_eq!(ctx.server().state().unwrap().map_name.as_deref(), Some(c""));
    MODE.set(9);
    assert_eq!(
        ctx.server().state().unwrap().map_name.unwrap().to_bytes(),
        &[0xff]
    );
    // Map retirement, nested lookup, transition on the second getter, shutdown.
    for mode in [1, 2, 6, 7] {
        MODE.set(mode);
        assert!(ctx.server().state().is_none());
        SHUTDOWN.set(false);
        crate::trace::activate_map();
    }
    assert!(NESTED_REJECTED.get());
    assert_eq!(first.map_name.as_deref(), Some(c"arena"));
    MODE.set(0);
    // SAFETY: Mutate through the retained raw pointer, modeling native writes.
    unsafe {
        (*globals).time = -2.0;
        (*globals).frametime = -0.1;
        (*globals).deathmatch = f32::INFINITY;
        (*globals).coop = f32::NAN;
        (*globals).teamplay = f32::NEG_INFINITY;
        (*globals).serverflags = f32::NAN;
        (*globals).mapname = 0;
        (*globals).startspot = 0;
        (*globals).maxEntities = 0;
    }
    let invalid = ctx.server().state().unwrap();
    assert_eq!(invalid.time, Some(-2.0));
    assert_eq!(invalid.frame_time, None);
    assert_eq!(
        (
            invalid.deathmatch,
            invalid.coop,
            invalid.teamplay,
            invalid.server_flags
        ),
        (None, None, None, None)
    );
    assert_eq!((invalid.max_entities, invalid.max_clients), (None, None));
    assert!(invalid.map_name.is_none() && invalid.start_spot.is_none());
    for (entities, clients) in [(8, 0), (8, -1), (8, 8), (8, i32::MAX), (65537, 1), (-1, 1)] {
        // SAFETY: The mock globals remain allocated on this thread.
        unsafe {
            (*globals).maxEntities = entities;
            (*globals).maxClients = clients;
            (*globals).time = f32::NAN;
            (*globals).frametime = f32::INFINITY;
        }
        let invalid = ctx.server().state().unwrap();
        assert!(invalid.max_clients.is_none());
        assert!(invalid.time.is_none() && invalid.frame_time.is_none());
    }
    // SAFETY: Same live mock allocation.
    unsafe {
        (*globals).maxEntities = 65536;
        (*globals).maxClients = 65535;
        (*globals).frametime = 0.0;
        (*globals).time = 0.0; // Engine clock reset is preserved without monotonicity assumptions.
    }
    let state = ctx.server().state().unwrap();
    assert_eq!(
        (state.max_entities, state.max_clients),
        (Some(65536), Some(65535))
    );
    assert_eq!((state.time, state.frame_time), (Some(0.0), Some(0.0)));
    ENGINE.set(Engine::default());
    // SAFETY: Same live mock allocation.
    unsafe { (*globals).mapname = 1 };
    assert!(ctx.server().state().unwrap().map_name.is_none());
    // SAFETY: Null clears the retained pointer while the map remains active.
    unsafe { crate::trace::set_globals(ptr::null_mut()) };
    assert!(ctx.server().state().is_none());
    crate::trace::invalidate_map();
    assert!(ctx.server().state().is_none());
    BUFFER.set(ptr::null_mut());
}
