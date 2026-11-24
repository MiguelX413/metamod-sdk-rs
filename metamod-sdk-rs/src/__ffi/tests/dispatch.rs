#[test]
fn synchronous_results_survive_reentry_and_panics_fall_back() {
    #[derive(Default)]
    struct TestPlugin;
    impl Plugin for TestPlugin {
        const METADATA: Metadata = METADATA;
    }

    queried();
    let mut globals = MetaGlobals::default();
    let mut functions = MetaFunctions::default();
    // SAFETY: The hook globals remain live throughout this test.
    assert_eq!(
        unsafe { attach::<TestPlugin>(PT_STARTUP, &mut functions, &raw mut globals) },
        1
    );
    for (decision, expected, value) in [
        (HookResult::Ignored, MRES_IGNORED, false),
        (HookResult::Handled, MRES_HANDLED, false),
        (HookResult::Override(true), MRES_OVERRIDE, true),
        (HookResult::Supercede(true), MRES_SUPERCEDE, true),
    ] {
        let returned = synchronous::<TestPlugin, _>(|_, _, _| {
            let skipped = synchronous::<TestPlugin, bool>(|_, _, _| panic!("nested hook ran"));
            assert!(!skipped);
            submit::<TestPlugin>(Event::Deactivate);
            decision
        });
        assert_eq!(globals.mres, expected);
        assert_eq!(returned, value);
    }
    assert!(!synchronous::<TestPlugin, bool>(|_, _, _| panic!(
        "hook panic"
    )));
    assert_eq!(globals.mres, MRES_IGNORED);
    // SAFETY: Shutdown uses the live hook globals on the attached thread.
    unsafe { game_shutdown::<TestPlugin>() };
}

#[derive(Default)]
struct BudgetPlugin {
    calls: usize,
    peak_pending: usize,
}

impl Plugin for BudgetPlugin {
    const METADATA: Metadata = METADATA;

    fn start_frame(&mut self, _: &mut Context<'_>) {
        self.calls += 1;
        for _ in 0..MAX_EVENTS_PER_DISPATCH {
            submit::<Self>(Event::Activate);
            let pending = EVENTS.with(|events| events.borrow().pending.len());
            self.peak_pending = self.peak_pending.max(pending);
        }
    }

    fn server_activate(&mut self, _: &mut Context<'_>) {
        self.calls += 1;
        submit::<Self>(Event::Activate);
    }
}

fn exercise_dispatch_budget(first: unsafe extern "C" fn(), expected: (usize, usize)) {
    queried();
    let mut globals = MetaGlobals::default();
    let globals_ptr = &raw mut globals;
    let mut functions = MetaFunctions::default();
    // SAFETY: All calls run on the attached thread with live globals.
    unsafe {
        assert_eq!(
            attach::<BudgetPlugin>(PT_STARTUP, &mut functions, globals_ptr),
            1
        );
        first();
    }
    assert_eq!(globals.mres, MRES_IGNORED);
    assert!(EVENTS.with(|events| events.borrow().pending.is_empty()));
    assert!(matches!(
        STATE.get(),
        State::Attached { disabled: true, .. }
    ));
    assert!(!RUNNING.get());

    // SAFETY: Use the same live native storage as the attached SDK.
    unsafe {
        (*globals_ptr).mres = MRES_UNSET;
        start_frame::<BudgetPlugin>();
    }
    assert_eq!(globals.mres, MRES_IGNORED);
    INSTANCE.with(|instance| {
        let slot = instance.borrow();
        let plugin = slot
            .as_ref()
            .unwrap()
            .downcast_ref::<BudgetPlugin>()
            .unwrap();
        assert_eq!((plugin.calls, plugin.peak_pending), expected);
    });
    // SAFETY: Shutdown runs on the attached thread with live globals.
    unsafe { game_shutdown::<BudgetPlugin>() };
    assert!(INSTANCE.with(|instance| instance.borrow().is_none()));
}

#[test]
fn dispatch_budget_bounds_a_burst_of_pending_events() {
    exercise_dispatch_budget(
        start_frame::<BudgetPlugin>,
        (1, MAX_EVENTS_PER_DISPATCH - 1),
    );
}

#[test]
fn dispatch_budget_stops_a_feedback_loop_with_a_small_queue() {
    unsafe extern "C" fn activate() {
        // SAFETY: The helper runs this on the attached thread.
        unsafe {
            begin_map(ptr::null_mut(), 0, 0);
            server_activate::<BudgetPlugin>(ptr::null_mut(), 0, 0);
        }
    }
    exercise_dispatch_budget(activate, (MAX_EVENTS_PER_DISPATCH, 0));
}

#[test]
fn nested_lifecycle_is_deferred_and_panic_disables_user_hooks() {
    #[derive(Default)]
    struct TestPlugin {
        trace: Vec<&'static str>,
    }

    impl Plugin for TestPlugin {
        const METADATA: Metadata = METADATA;

        fn start_frame(&mut self, ctx: &mut Context<'_>) {
            self.trace.push("frame");
            let slot = const { PlayerSlot::new(7).unwrap() };
            let player = EVENTS
                .with(|events| events.borrow_mut().join(slot, 0))
                .unwrap();
            assert!(ctx.is_player_current(player));
            // SAFETY: These synthetic hooks run on the attached thread.
            unsafe {
                start_frame::<Self>();
                begin_map(ptr::null_mut(), 0, 0);
                server_activate::<Self>(ptr::null_mut(), 0, 0);
                server_deactivate::<Self>();
            }
            assert!(!ctx.is_player_current(player));
            self.trace.push("frame end");
        }

        fn server_deactivate(&mut self, _: &mut Context<'_>) {
            self.trace.push("deactivate");
            panic!("disable all callbacks");
        }

        fn server_activate(&mut self, _: &mut Context<'_>) {
            self.trace.push("activate");
        }
    }

    queried();
    let mut globals = MetaGlobals {
        prev_mres: MRES_HANDLED,
        ..Default::default()
    };
    let globals_ptr = &raw mut globals;
    let mut functions = MetaFunctions::default();
    // SAFETY: The test keeps its foreign output storage live.
    unsafe {
        assert_eq!(
            attach::<TestPlugin>(PT_STARTUP, &mut functions, globals_ptr),
            1
        );
        thread::spawn(|| start_frame::<TestPlugin>())
            .join()
            .unwrap();
        assert_eq!(globals.mres, MRES_UNSET);
        start_frame::<TestPlugin>();
        assert_eq!(globals.mres, MRES_IGNORED);
        assert_eq!(globals.prev_mres, MRES_HANDLED);
        // Use the same native pointer for writes between invocations.
        (*globals_ptr).mres = MRES_UNSET;
        begin_map(ptr::null_mut(), 0, 0);
        server_activate::<TestPlugin>(ptr::null_mut(), 0, 0);
    }
    assert_eq!(globals.mres, MRES_IGNORED);
    INSTANCE.with(|instance| {
        let slot = instance.borrow();
        let plugin = slot.as_ref().unwrap().downcast_ref::<TestPlugin>().unwrap();
        assert_eq!(
            plugin.trace,
            ["frame", "frame end", "activate", "deactivate"]
        );
    });
    assert!(matches!(
        STATE.get(),
        State::Attached { disabled: true, .. }
    ));
    // SAFETY: Shutdown is delivered on the same thread with live globals.
    unsafe { game_shutdown::<TestPlugin>() };
    assert!(INSTANCE.with(|instance| instance.borrow().is_none()));
}

