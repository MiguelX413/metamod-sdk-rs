#[test]
fn fake_clients_wait_for_release_and_reject_reentrant_invalidation() {
    use crate::{FakeClient, FakeClientError, FakeClientState};

    thread_local! {
        static EDICT: Cell<*mut edict_t> = const { Cell::new(ptr::null_mut()) };
        static USER: Cell<c_int> = const { Cell::new(1) };
        static MODE: Cell<u8> = const { Cell::new(0) };
        static PHASE: Cell<u8> = const { Cell::new(0) };
        static HANDLE: RefCell<Option<FakeClient>> = const { RefCell::new(None) };
        static FRAME_CALLS: Cell<u8> = const { Cell::new(0) };
        static KICK_MATCHED: Cell<bool> = const { Cell::new(false) };
    }

    unsafe extern "C" fn allocate(_: *const c_char) -> *mut edict_t {
        if MODE.get() == 2 {
            // SAFETY: The mock keeps the native hook globals live.
            unsafe { server_deactivate::<Fixture>() };
            return ptr::dangling_mut();
        }
        EDICT.get()
    }
    unsafe extern "C" fn index(_: *const edict_t) -> c_int {
        1
    }
    unsafe extern "C" fn entity(_: c_int) -> *mut edict_t {
        if MODE.get() == 3 {
            MODE.set(0);
            // SAFETY: This mock keeps its hook globals live during reentry.
            unsafe { start_frame::<Fixture>() };
        }
        EDICT.get()
    }
    unsafe extern "C" fn userid(_: *mut edict_t) -> c_int {
        USER.get()
    }
    unsafe extern "C" fn connect(
        entity: *mut edict_t,
        _: *const c_char,
        _: *const c_char,
        _: *mut c_char,
    ) -> c_int {
        if MODE.get() == 1 {
            // SAFETY: This mock reenters with the same live edict and hook globals.
            unsafe { player_disconnect::<Fixture>(entity) };
        }
        1
    }
    unsafe extern "C" fn put(_: *mut edict_t) {}
    unsafe extern "C" fn queue(command: *const c_char) {
        // SAFETY: The SDK supplies a live, terminated command during this call.
        KICK_MATCHED.set(unsafe { CStr::from_ptr(command) } == c"kick #1\n");
    }

    #[derive(Default)]
    struct Fixture;
    impl Plugin for Fixture {
        const METADATA: Metadata = METADATA;

        fn start_frame(&mut self, ctx: &mut Context<'_>) {
            let server = ctx.server();
            // SAFETY: This controlled fixture provides all allocation, setup and kick callbacks.
            let create = || unsafe {
                server.create_fake_client(c"probe", |setup| {
                    setup.connect(c"loopback") && setup.put_in_server()
                })
            };
            match PHASE.get() {
                0 => {
                    let client = create().unwrap();
                    assert_eq!(ctx.player_at(client.player().slot()), Some(client.player()));
                    server.request_fake_client_removal(&client).unwrap();
                    assert_eq!(client.state(), FakeClientState::RemovalQueued);
                    HANDLE.with(|handle| handle.replace(Some(client)));
                }
                1 => assert!(matches!(create(), Err(FakeClientError::SetupIncomplete))),
                2 => assert!(matches!(create(), Err(FakeClientError::SetupIncomplete))),
                3 => FRAME_CALLS.update(|calls| calls + 1),
                _ => unreachable!(),
            }
        }
    }

    queried();
    ENGINE.set(Engine {
        create_fake_client: Some(allocate),
        queue_server_command: Some(queue),
        index: Some(index),
        entity: Some(entity),
        userid: Some(userid),
        ..Default::default()
    });
    let mut edict = edict_t::default();
    EDICT.set(&raw mut edict);
    let mut engine_globals = EngineGlobals {
        maxClients: 2,
        maxEntities: 8,
        ..Default::default()
    };
    let mut globals = MetaGlobals::default();
    let mut functions = MetaFunctions::default();
    let mut game = DllFunctions {
        pfnClientConnect: Some(connect),
        pfnClientPutInServer: Some(put),
        ..Default::default()
    };
    let mut game_tables = GameDllFunctions {
        dllapi_table: &mut game,
        ..Default::default()
    };
    // SAFETY: All tables remain allocated until after shutdown, and the
    // deliberately stale return is guarded by an observed map change.
    unsafe {
        assert_eq!(
            attach::<Fixture>(PT_STARTUP, &mut functions, &raw mut globals),
            1
        );
        capture_game_dll(&mut game_tables);
        crate::trace::set_globals(&raw mut engine_globals);
        crate::trace::activate_map();
        start_frame::<Fixture>();
        assert!(KICK_MATCHED.get());
        let client = HANDLE.with(|handle| handle.take()).unwrap();
        player_disconnect::<Fixture>(EDICT.get());
        crate::fake_client::poll_removals();
        assert_eq!(client.state(), FakeClientState::RemovalQueued);
        assert_eq!(player_at(client.player().slot()), None);
        USER.set(2);
        PHASE.set(3);
        MODE.set(3);
        start_frame::<Fixture>();
        assert_eq!(FRAME_CALLS.get(), 1);
        assert_eq!(client.state(), FakeClientState::Removed);

        PHASE.set(1);
        MODE.set(1);
        start_frame::<Fixture>();
        assert_eq!(player_at(client.player().slot()), None);
        PHASE.set(2);
        MODE.set(2);
        start_frame::<Fixture>();
        assert_eq!(player_at(client.player().slot()), None);
        assert!(
            registration_available(),
            "plugin assertions must not have panicked"
        );
        assert_eq!(client.state(), FakeClientState::Removed);
        crate::trace::activate_map();
        PHASE.set(0);
        MODE.set(0);
        USER.set(1);
        start_frame::<Fixture>();
        let replacement = HANDLE.with(|handle| handle.take()).unwrap();
        assert_eq!(replacement.player().slot(), client.player().slot());
        assert_ne!(replacement.player(), client.player());
        server_deactivate::<Fixture>();
        assert_eq!(replacement.state(), FakeClientState::Invalidated);
        game_shutdown::<Fixture>();
    }
}

