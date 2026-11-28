#[test]
fn precaching_is_scoped_retains_static_names_and_invalidates_map_indices() {
    use crate::{ModelIndex, Precache, PrecacheError, ResourcePath, SoundIndex};
    const MODEL: &ResourcePath = ResourcePath::new(c"sprites/laserbeam.spr").unwrap();
    const SOUND: &ResourcePath = ResourcePath::new(c"common/null.wav").unwrap();
    thread_local! {
        static WORLD: Cell<*mut edict_t> = const { Cell::new(ptr::null_mut()) };
        static MODE: Cell<u8> = const { Cell::new(0) };
        static LOOKUP_MODE: Cell<u8> = const { Cell::new(0) };
        static MODEL_RESULT: Cell<Option<Result<ModelIndex, PrecacheError>>> = const { Cell::new(None) };
        static SOUND_RESULT: Cell<Option<Result<SoundIndex, PrecacheError>>> = const { Cell::new(None) };
        static NAME: Cell<*const c_char> = const { Cell::new(ptr::null()) };
        static CALLBACKS: Cell<usize> = const { Cell::new(0) };
        static SOUNDS: Cell<usize> = const { Cell::new(0) };
        static DROPS: Cell<usize> = const { Cell::new(0) };
    }
    unsafe extern "C" fn entity(_: c_int) -> *mut edict_t {
        match LOOKUP_MODE.replace(0) {
            // SAFETY: Callback globals remain live. Nested identification must
            // not recurse into this getter or borrow the plugin twice.
            1 => {
                unsafe { spawn::<Fixture>(WORLD.get()) };
            }
            2 => {
                unsafe { server_deactivate::<Fixture>() };
                return ptr::dangling_mut();
            }
            _ => {}
        }
        WORLD.get()
    }
    unsafe extern "C" fn model(name: *const c_char) -> c_int {
        NAME.set(name);
        // SAFETY: The SDK supplies the tested static path for this entire call
        // and for retention afterwards, including shutdown during reentry.
        assert_eq!(unsafe { CStr::from_ptr(name) }, MODEL.as_c_str());
        match MODE.get() {
            // SAFETY: The fixture retains all native callback/result storage.
            1 => unsafe { server_deactivate::<Fixture>() },
            2 => {
                unsafe { spawn::<Fixture>(WORLD.get()) };
            }
            3 => unsafe {
                begin_map(ptr::null_mut(), 0, 0);
                server_activate::<Fixture>(ptr::null_mut(), 0, 0);
            },
            4 => {
                unsafe { game_shutdown::<Fixture>() };
                assert_eq!(DROPS.get(), 0, "active plugin must not be dropped");
            }
            6 => return 0,
            7 => return -1,
            _ => {}
        }
        // The input remains valid after synchronous nested shutdown.
        assert_eq!(unsafe { CStr::from_ptr(name) }, MODEL.as_c_str());
        11
    }
    unsafe extern "C" fn sound(name: *const c_char) -> c_int {
        // SAFETY: The SDK supplies the static sound path for this call.
        assert_eq!(unsafe { CStr::from_ptr(name) }, SOUND.as_c_str());
        SOUNDS.update(|n| n + 1);
        match MODE.get() {
            8 => return 0,
            9 => {
                // SAFETY: Mock callback and result storage remain allocated.
                unsafe { server_deactivate::<Fixture>() };
            }
            10 => return -1,
            _ => {}
        }
        12
    }
    #[derive(Default)]
    struct Fixture;
    impl Drop for Fixture {
        fn drop(&mut self) {
            DROPS.update(|n| n + 1);
        }
    }
    impl Plugin for Fixture {
        const METADATA: Metadata = METADATA;
        const PRECACHE: bool = true;
        fn precache(&mut self, resources: &mut Precache<'_>) {
            CALLBACKS.update(|n| n + 1);
            MODEL_RESULT.set(Some(resources.model(MODEL)));
            SOUND_RESULT.set(Some(resources.sound(SOUND)));
        }
    }
    queried();
    crate::resources::invalidate();
    ENGINE.set(Engine {
        entity: Some(entity),
        precache_model: Some(model),
        precache_sound: Some(sound),
        ..Default::default()
    });
    let mut world = edict_t::default();
    WORLD.set(&raw mut world);
    let mut native = EngineGlobals {
        maxEntities: 8,
        maxClients: 2,
        ..Default::default()
    };
    let mut globals = MetaGlobals::default();
    let mut functions = MetaFunctions::default();
    // SAFETY: All native tables, globals and world storage outlive callbacks.
    unsafe {
        crate::trace::set_globals(&raw mut native);
        assert_eq!(
            attach::<Fixture>(PT_STARTUP, &mut functions, &raw mut globals),
            1
        );
        assert_eq!(spawn::<Fixture>(ptr::null_mut()), 0);
        assert_eq!(CALLBACKS.get(), 0);
        LOOKUP_MODE.set(2);
        spawn::<Fixture>(WORLD.get());
        assert_eq!(
            CALLBACKS.get(),
            0,
            "retired world identification must be discarded"
        );
        LOOKUP_MODE.set(1);
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
        spawn::<Fixture>(WORLD.get());
        assert_eq!(globals.mres, MRES_IGNORED);
        let model_id = MODEL_RESULT.get().unwrap().unwrap();
        let sound_id = SOUND_RESULT.get().unwrap().unwrap();
        assert_eq!((model_id.get(), sound_id.get()), (11, 12));
        assert!(!ctx.is_model_current(model_id));
        begin_map(ptr::null_mut(), 0, 0);
        server_activate::<Fixture>(ptr::null_mut(), 0, 0);
        assert!(ctx.is_model_current(model_id) && ctx.is_sound_current(sound_id));
        spawn::<Fixture>(WORLD.get()); // Runtime/duplicate world spawn cannot reopen.
        assert_eq!(CALLBACKS.get(), 1);
        for mode in [1, 2, 3, 6, 7, 8, 9, 10] {
            server_deactivate::<Fixture>();
            assert!(!ctx.is_model_current(model_id));
            MODE.set(mode);
            let sounds_before = SOUNDS.get();
            spawn::<Fixture>(WORLD.get());
            if mode <= 3 {
                assert_eq!(MODEL_RESULT.get(), Some(Err(PrecacheError::Unavailable)));
                assert_eq!(SOUND_RESULT.get(), Some(Err(PrecacheError::Unavailable)));
                assert_eq!(SOUNDS.get(), sounds_before);
            } else if mode == 6 || mode == 7 {
                assert_eq!(MODEL_RESULT.get(), Some(Err(PrecacheError::Rejected)));
            } else {
                assert!(matches!(MODEL_RESULT.get(), Some(Ok(_))));
                let expected = if mode == 9 {
                    PrecacheError::Unavailable
                } else {
                    PrecacheError::Rejected
                };
                assert_eq!(SOUND_RESULT.get(), Some(Err(expected)));
            }
            assert!(
                registration_available(),
                "callback assertions must not panic"
            );
            assert_eq!(globals.mres, MRES_IGNORED);
        }
        server_deactivate::<Fixture>();
        MODE.set(0);
        ENGINE.update(|engine| Engine {
            precache_sound: None,
            ..engine
        });
        spawn::<Fixture>(WORLD.get());
        assert_eq!(SOUND_RESULT.get(), Some(Err(PrecacheError::Unsupported)));
        let replacement = MODEL_RESULT.get().unwrap().unwrap();
        assert_ne!(model_id, replacement);
        begin_map(ptr::null_mut(), 0, 0);
        server_activate::<Fixture>(ptr::null_mut(), 0, 0);
        assert!(ctx.is_model_current(replacement));
        assert!(!ctx.is_model_current(model_id));
        // Another activation without an intervening worldspawn fails closed.
        begin_map(ptr::null_mut(), 0, 0);
        server_activate::<Fixture>(ptr::null_mut(), 0, 0);
        assert!(!ctx.is_model_current(replacement));
        let callbacks_before = CALLBACKS.get();
        spawn::<Fixture>(WORLD.get());
        assert_eq!(
            CALLBACKS.get(),
            callbacks_before,
            "an active map cannot grant a new loading scope"
        );
        server_deactivate::<Fixture>();
        MODE.set(4);
        spawn::<Fixture>(WORLD.get());
        assert_eq!(DROPS.get(), 1);
        assert_eq!(MODEL_RESULT.get(), Some(Err(PrecacheError::Unavailable)));
        assert_eq!(globals.mres, MRES_IGNORED);
        assert_eq!(CStr::from_ptr(NAME.get()), MODEL.as_c_str());
        crate::trace::set_globals(ptr::null_mut());
    }
}

#[test]
fn precache_panics_are_contained_and_scope_is_closed() {
    use crate::Precache;
    #[derive(Default)]
    struct Fixture;
    impl Plugin for Fixture {
        const METADATA: Metadata = METADATA;
        const PRECACHE: bool = true;
        fn precache(&mut self, _: &mut Precache<'_>) {
            panic!("precache fixture panic");
        }
    }
    thread_local! { static WORLD: Cell<*mut edict_t> = const { Cell::new(ptr::null_mut()) }; }
    unsafe extern "C" fn entity(_: c_int) -> *mut edict_t {
        WORLD.get()
    }
    queried();
    crate::resources::invalidate();
    ENGINE.set(Engine {
        entity: Some(entity),
        ..Default::default()
    });
    let mut world = edict_t::default();
    WORLD.set(&raw mut world);
    let mut globals = MetaGlobals::default();
    let mut functions = MetaFunctions::default();
    // SAFETY: The mock world and Metamod result storage remain live throughout.
    unsafe {
        assert_eq!(
            attach::<Fixture>(PT_STARTUP, &mut functions, &raw mut globals),
            1
        );
        spawn::<Fixture>(WORLD.get());
        assert_eq!(globals.mres, MRES_IGNORED);
        assert!(!registration_available());
        spawn::<Fixture>(WORLD.get());
        assert_eq!(globals.mres, MRES_IGNORED);
        game_shutdown::<Fixture>();
    }
}
