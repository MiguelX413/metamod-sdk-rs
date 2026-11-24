#[test]
fn rejected_native_cvar_is_attempted_only_once() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static ATTEMPTS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn missing(_: *const c_char) -> *mut raw::cvar_t {
        ptr::null_mut()
    }

    unsafe extern "C" fn reject(_: *mut raw::cvar_t) {
        ATTEMPTS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: This synthetic reentry runs with live hook globals.
        unsafe { start_frame::<TestPlugin>() };
    }

    #[derive(Default)]
    struct TestPlugin;
    impl Plugin for TestPlugin {
        const METADATA: Metadata = METADATA;
        fn start_frame(&mut self, context: &mut Context<'_>) {
            for name in [
                crate::console_name!(c"test_rejected"),
                crate::console_name!(c"TEST_REJECTED"),
                crate::console_name!(c"test_rejected"),
            ] {
                assert_eq!(
                    context
                        .server()
                        .register_cvar(name, crate::cvar_value!(c"value")),
                    Err(crate::EngineError::RegistrationRejected)
                );
            }
        }
    }
    queried();
    ENGINE.set(Engine {
        find_cvar: Some(missing),
        register_cvar: Some(reject),
        ..Default::default()
    });
    let mut globals = MetaGlobals::default();
    let mut functions = MetaFunctions::default();
    // SAFETY: All invocations run on the attached thread with live globals.
    unsafe {
        assert_eq!(
            attach::<TestPlugin>(PT_STARTUP, &mut functions, &raw mut globals),
            1
        );
        start_frame::<TestPlugin>();
        assert!(matches!(
            STATE.get(),
            State::Attached {
                disabled: false,
                ..
            }
        ));
        game_shutdown::<TestPlugin>();
    }
    assert_eq!(ATTEMPTS.load(Ordering::Relaxed), 1);
}

#[test]
fn cvar_getter_discards_a_result_invalidated_during_native_reentry() {
    unsafe extern "C" fn find(_: *const c_char) -> *mut raw::cvar_t {
        // The presence check treats this address as an opaque value.
        ptr::dangling_mut()
    }

    unsafe extern "C" fn get(_: *const c_char) -> *const c_char {
        let value = std::ffi::CString::new("original value").unwrap();
        let result = value.as_ptr();
        // Model a native post hook that reenters the SDK and replaces the
        // CVar value after the engine has selected its return pointer.
        unsafe { start_frame::<TestPlugin>() };
        drop(value);
        result
    }

    #[derive(Default)]
    struct TestPlugin;
    impl Plugin for TestPlugin {
        const METADATA: Metadata = METADATA;

        fn start_frame(&mut self, context: &mut Context<'_>) {
            assert_eq!(
                context.server().cvar(crate::console_name!(c"test_reentry")),
                Err(crate::EngineError::Unavailable)
            );
        }
    }

    queried();
    ENGINE.set(Engine {
        find_cvar: Some(find),
        get_cvar: Some(get),
        ..Default::default()
    });
    let mut globals = MetaGlobals::default();
    let mut functions = MetaFunctions::default();
    // SAFETY: The hook globals remain live through nested dispatch.
    unsafe {
        assert_eq!(
            attach::<TestPlugin>(PT_STARTUP, &mut functions, &raw mut globals),
            1
        );
        start_frame::<TestPlugin>();
        assert!(registration_available());
        game_shutdown::<TestPlugin>();
    }
}

#[test]
fn cvar_lookup_interrupted_by_shutdown_discards_native_result() {
    unsafe extern "C" fn find(_: *const c_char) -> *mut raw::cvar_t {
        // SAFETY: This test attaches on this thread with live globals.
        unsafe { game_shutdown::<TestPlugin>() };
        ptr::null_mut()
    }

    unsafe extern "C" fn get(_: *const c_char) -> *const c_char {
        ptr::null()
    }

    #[derive(Default)]
    struct TestPlugin;
    impl Plugin for TestPlugin {
        const METADATA: Metadata = METADATA;

        fn start_frame(&mut self, context: &mut Context<'_>) {
            assert_eq!(
                context
                    .server()
                    .cvar(crate::console_name!(c"test_shutdown")),
                Err(crate::EngineError::Unavailable)
            );
        }
    }

    queried();
    ENGINE.set(Engine {
        find_cvar: Some(find),
        get_cvar: Some(get),
        ..Default::default()
    });
    let mut globals = MetaGlobals::default();
    let mut functions = MetaFunctions::default();
    // SAFETY: The native tables remain live on this thread through shutdown.
    unsafe {
        assert_eq!(
            attach::<TestPlugin>(PT_STARTUP, &mut functions, &raw mut globals),
            1
        );
        start_frame::<TestPlugin>();
    }
    assert!(SHUTDOWN.get());
    assert!(INSTANCE.with(|instance| instance.borrow().is_none()));
    assert!(matches!(
        STATE.get(),
        State::Attached {
            disabled: false,
            ..
        }
    ));
    assert_eq!(globals.mres, MRES_IGNORED);
}

