#[test]
fn native_print_reentry_keeps_the_plugin_and_borrowed_buffer_live() {
    thread_local! {
        static DROPS: Cell<usize> = const { Cell::new(0) };
    }

    unsafe extern "C" fn print(message: *const c_char) {
        // SAFETY: The SDK lends the mock a C string for this entire call.
        let before = unsafe { engine::copy_string(message, 16) };
        // SAFETY: The test owns the hook globals until both callbacks return.
        unsafe { game_shutdown::<TestPlugin>() };
        let after = unsafe { engine::copy_string(message, 16) };
        // Record the observation without panicking inside the C ABI.
        OBSERVED.set(before.as_deref() == Some(c"owned by plugin") && before == after);
    }

    thread_local! {
        static OBSERVED: Cell<bool> = const { Cell::new(false) };
    }

    struct TestPlugin(std::ffi::CString);

    impl Default for TestPlugin {
        fn default() -> Self {
            Self(c"owned by plugin".to_owned())
        }
    }

    impl Plugin for TestPlugin {
        const METADATA: Metadata = METADATA;

        fn start_frame(&mut self, context: &mut Context<'_>) {
            context.server().print(&self.0);
            assert_eq!(DROPS.get(), 0);
            assert!(!engine_available());
            self.0 = c"frame finished".to_owned();
        }
    }

    impl Drop for TestPlugin {
        fn drop(&mut self) {
            DROPS.update(|count| count + 1);
        }
    }

    queried();
    ENGINE.set(Engine {
        print: Some(print),
        ..Default::default()
    });
    let mut globals = MetaGlobals::default();
    let mut functions = MetaFunctions::default();
    // SAFETY: Both native tables stay live through all mock calls.
    unsafe {
        assert_eq!(
            attach::<TestPlugin>(PT_STARTUP, &mut functions, &raw mut globals),
            1
        );
        start_frame::<TestPlugin>();
        game_shutdown::<TestPlugin>();
    }
    assert!(OBSERVED.get());
    assert_eq!(DROPS.get(), 1);
    assert!(INSTANCE.with(|instance| instance.borrow().is_none()));
    assert_eq!(globals.mres, MRES_IGNORED);
}

#[test]
fn nested_shutdown_revokes_engine_access_and_cleans_up_once() {
    #[derive(Default)]
    struct TestPlugin;

    thread_local! {
        static TRACE: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    }

    impl Plugin for TestPlugin {
        const METADATA: Metadata = METADATA;

        fn start_frame(&mut self, _: &mut Context<'_>) {
            assert!(engine_available());
            // SAFETY: The test supplies live globals on this thread.
            unsafe { game_shutdown::<Self>() };
            assert!(!engine_available());
            TRACE.with(|trace| trace.borrow_mut().push("frame end"));
        }

        fn shutdown(&mut self) {
            TRACE.with(|trace| trace.borrow_mut().push("shutdown"));
            panic!("cleanup panic");
        }
    }

    impl Drop for TestPlugin {
        fn drop(&mut self) {
            TRACE.with(|trace| trace.borrow_mut().push("drop"));
            panic!("destructor panic");
        }
    }

    queried();
    let mut globals = MetaGlobals::default();
    let globals_ptr = &raw mut globals;
    let mut functions = MetaFunctions::default();
    // SAFETY: All calls use the attached thread and live globals.
    unsafe {
        assert_eq!(
            attach::<TestPlugin>(PT_STARTUP, &mut functions, globals_ptr),
            1
        );
        start_frame::<TestPlugin>();
        assert_eq!(globals.mres, MRES_IGNORED);
        // Use the same native pointer for writes between invocations.
        (*globals_ptr).mres = MRES_UNSET;
        game_shutdown::<TestPlugin>();
        assert_eq!(globals.mres, MRES_IGNORED);
    }
    TRACE.with(|trace| assert_eq!(*trace.borrow(), ["frame end", "shutdown", "drop"]));
    assert!(INSTANCE.with(|instance| instance.borrow().is_none()));
}

#[test]
fn failed_attach_drops_instance_and_allows_retry() {
    #[derive(Default)]
    struct TestPlugin;

    thread_local! {
        static DROPS: Cell<usize> = const { Cell::new(0) };
        static PAYLOAD_DROPS: Cell<usize> = const { Cell::new(0) };
    }

    struct Payload;
    impl Drop for Payload {
        fn drop(&mut self) {
            PAYLOAD_DROPS.set(PAYLOAD_DROPS.get() + 1);
            panic!("panic payload destructor failed");
        }
    }

    impl Plugin for TestPlugin {
        const METADATA: Metadata = METADATA;

        fn initialize(&mut self, _: &mut Context<'_>) -> bool {
            if DROPS.get() == 0 {
                std::panic::panic_any(Payload);
            }
            false
        }
    }

    impl Drop for TestPlugin {
        fn drop(&mut self) {
            DROPS.set(DROPS.get() + 1);
            panic!("destructor panic");
        }
    }

    queried();
    let mut globals = MetaGlobals::default();
    let mut functions = MetaFunctions::default();
    for drops in 1..=2 {
        // SAFETY: The output pointers remain live during the call.
        assert_eq!(
            unsafe { attach::<TestPlugin>(PT_STARTUP, &mut functions, &raw mut globals) },
            0
        );
        assert_eq!(DROPS.get(), drops);
        assert_eq!(PAYLOAD_DROPS.get(), 1);
        assert!(matches!(STATE.get(), State::Queried(_)));
        assert!(functions.pfnGetEntityAPI2.is_none());
    }
    STATE.set(State::Unqueried);
}
