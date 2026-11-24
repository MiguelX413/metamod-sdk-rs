#[test]
fn client_admission_is_synchronous_and_bounds_the_rejection_buffer() {
    #[derive(Default)]
    struct Admission;
    impl Plugin for Admission {
        const METADATA: Metadata = METADATA;
        const CLIENT_CONNECT_PRE: bool = true;

        fn client_connect<'a>(
            &'a mut self,
            _: &mut Context<'_>,
            client: &'a ClientConnection,
        ) -> ClientAdmission<'a> {
            assert_eq!(client.address.as_c_str(), c"loopback");
            match client.name.as_bytes() {
                b"defer" => ClientAdmission::Defer,
                b"empty" => ClientAdmission::reject(c"").unwrap(),
                b"raw" => ClientAdmission::reject(c"\xff").unwrap(),
                b"panic" => panic!("admission panic"),
                b"exact" => {
                    ClientAdmission::reject_owned(std::ffi::CString::new(vec![b'x'; 127]).unwrap())
                        .unwrap()
                }
                b"utf8" => ClientAdmission::reject_trunc(
                    &std::ffi::CString::new(format!("{}é", "x".repeat(126))).unwrap(),
                ),
                _ => {
                    let mut nested = [42; 128];
                    // SAFETY: The nested call has live inputs and writable output.
                    let result = unsafe {
                        client_connect::<Self>(
                            ptr::null_mut(),
                            c"nested".as_ptr(),
                            c"loopback".as_ptr(),
                            nested.as_mut_ptr(),
                        )
                    };
                    assert_eq!(result, 1);
                    assert_eq!(nested, [42; 128]);
                    ClientAdmission::reject_trunc(&std::ffi::CString::new(vec![b'x'; 200]).unwrap())
                }
            }
        }
    }

    queried();
    let mut globals = MetaGlobals::default();
    let mut functions = MetaFunctions::default();
    let mut hooks = DllFunctions::default();
    let mut version = ENTITY_INTERFACE;
    // SAFETY: The mock keeps the hook globals and all ABI arguments live.
    unsafe {
        assert_eq!(
            attach::<Admission>(PT_STARTUP, &mut functions, &raw mut globals),
            1
        );
        assert_eq!(entity_api::<Admission>(&mut hooks, &mut version), 1);
        let connect = hooks.pfnClientConnect.unwrap();
        for (step, name) in [
            c"defer".as_ptr(),
            ptr::null(),
            c"reject".as_ptr(),
            c"exact".as_ptr(),
            c"utf8".as_ptr(),
            c"empty".as_ptr(),
            c"raw".as_ptr(),
            c"panic".as_ptr(),
            c"reject".as_ptr(),
        ]
        .into_iter()
        .enumerate()
        {
            let mut output = [42; 129];
            let result = connect(
                ptr::null_mut(),
                name,
                c"loopback".as_ptr(),
                output.as_mut_ptr(),
            );
            if (2..=4).contains(&step) {
                assert_eq!((result, globals.mres), (0, MRES_SUPERCEDE));
                assert!(output[..126].iter().all(|&byte| byte == b'x' as c_char));
                assert_eq!(output[126] as u8, if step == 4 { 0xc3 } else { b'x' });
                assert_eq!(output[127..], [0, 42]);
            } else if step == 5 {
                assert_eq!((result, globals.mres), (0, MRES_SUPERCEDE));
                assert_eq!(output[0], 0);
                assert_eq!(output[1..], [42; 128]);
            } else if step == 6 {
                assert_eq!((result, globals.mres), (0, MRES_SUPERCEDE));
                assert_eq!(output[0] as u8, 0xff);
                assert_eq!(output[1], 0);
                assert_eq!(output[2..], [42; 127]);
            } else {
                assert_eq!((result, globals.mres), (1, MRES_IGNORED));
                assert_eq!(output, [42; 129]);
            }
        }
        game_shutdown::<Admission>();
    }
}

#[test]
fn client_admission_discards_rejection_after_nested_shutdown() {
    thread_local! {
        static DROPPED: Cell<bool> = const { Cell::new(false) };
    }

    #[derive(Default)]
    struct Admission {
        reason: std::ffi::CString,
    }
    impl Plugin for Admission {
        const METADATA: Metadata = METADATA;

        fn client_connect<'a>(
            &'a mut self,
            _: &mut Context<'_>,
            _: &'a ClientConnection,
        ) -> ClientAdmission<'a> {
            self.reason = c"too late".to_owned();
            // SAFETY: This mock keeps the attached hook globals live.
            unsafe { game_shutdown::<Self>() };
            assert!(!DROPPED.get());
            ClientAdmission::reject(&self.reason).unwrap()
        }
    }

    impl Drop for Admission {
        fn drop(&mut self) {
            DROPPED.set(true);
        }
    }

    queried();
    let mut globals = MetaGlobals::default();
    let mut functions = MetaFunctions::default();
    let mut reason = [42; 128];
    // SAFETY: Mock globals, input strings and output remain live throughout.
    unsafe {
        assert_eq!(
            attach::<Admission>(PT_STARTUP, &mut functions, &raw mut globals),
            1
        );
        assert_eq!(
            client_connect::<Admission>(
                ptr::null_mut(),
                c"client".as_ptr(),
                c"loopback".as_ptr(),
                reason.as_mut_ptr()
            ),
            1
        );
    }
    assert!(DROPPED.get());
    assert_eq!(globals.mres, MRES_IGNORED);
    assert_eq!(reason, [42; 128]);
}

#[test]
fn borrowed_admission_is_snapshotted_before_deferred_mutation_and_shutdown() {
    thread_local! { static DROPPED: Cell<bool> = const { Cell::new(false) }; }
    struct Admission {
        reason: std::ffi::CString,
        shutdown: bool,
    }
    impl Default for Admission {
        fn default() -> Self {
            Self {
                reason: c"before".to_owned(),
                shutdown: false,
            }
        }
    }
    impl Plugin for Admission {
        const METADATA: Metadata = METADATA;
        fn client_connect<'a>(
            &'a mut self,
            ctx: &mut Context<'_>,
            client: &'a ClientConnection,
        ) -> ClientAdmission<'a> {
            self.shutdown = client.name.as_c_str() == c"shutdown";
            ctx.server().print(c"queue map callback");
            assert!(!DROPPED.get());
            ClientAdmission::reject(&self.reason).unwrap()
        }
        fn server_activate(&mut self, _: &mut Context<'_>) {
            if self.shutdown {
                // SAFETY: The fixture retains its hook globals through every callback.
                unsafe { game_shutdown::<Self>() };
            } else {
                // Free the allocation that the admission decision borrowed.
                self.reason = c"a replacement rejection reason".to_owned();
            }
        }
    }
    impl Drop for Admission {
        fn drop(&mut self) {
            DROPPED.set(true);
        }
    }
    unsafe extern "C" fn reenter(_: *const c_char) {
        // SAFETY: These paired native hooks use the fixture's live globals.
        unsafe {
            begin_map(ptr::null_mut(), 0, 0);
            server_activate::<Admission>(ptr::null_mut(), 0, 0);
        }
    }
    queried();
    ENGINE.set(Engine {
        print: Some(reenter),
        ..Default::default()
    });
    let mut globals = MetaGlobals::default();
    let mut functions = MetaFunctions::default();
    // SAFETY: Hook globals, tables, inputs and the 128-byte output stay allocated.
    unsafe {
        assert_eq!(
            attach::<Admission>(PT_STARTUP, &mut functions, &raw mut globals),
            1
        );
        let mut output = [42; 129];
        assert_eq!(
            client_connect::<Admission>(
                ptr::null_mut(),
                c"replace".as_ptr(),
                c"loopback".as_ptr(),
                output.as_mut_ptr()
            ),
            0
        );
        assert_eq!(globals.mres, MRES_SUPERCEDE);
        assert_eq!(
            &output[..7],
            &[
                b'b' as c_char,
                b'e' as c_char,
                b'f' as c_char,
                b'o' as c_char,
                b'r' as c_char,
                b'e' as c_char,
                0
            ]
        );
        assert_eq!(output[7..], [42; 122]);
        assert!(!DROPPED.get());
        output.fill(42);
        assert_eq!(
            client_connect::<Admission>(
                ptr::null_mut(),
                c"shutdown".as_ptr(),
                c"loopback".as_ptr(),
                output.as_mut_ptr()
            ),
            1
        );
        assert!(DROPPED.get());
        assert_eq!(globals.mres, MRES_IGNORED);
        assert_eq!(output, [42; 129]);
    }
}

#[test]
fn borrowed_admission_is_discarded_after_deferred_panic() {
    #[derive(Default)]
    struct Admission {
        reason: std::ffi::CString,
    }
    impl Plugin for Admission {
        const METADATA: Metadata = METADATA;
        fn client_connect<'a>(
            &'a mut self,
            _: &mut Context<'_>,
            _: &'a ClientConnection,
        ) -> ClientAdmission<'a> {
            self.reason = c"must be discarded".to_owned();
            submit::<Self>(Event::Activate);
            ClientAdmission::reject(&self.reason).unwrap()
        }
        fn server_activate(&mut self, _: &mut Context<'_>) {
            self.reason = c"changed".to_owned();
            panic!("deferred callback panic");
        }
    }
    queried();
    let mut globals = MetaGlobals::default();
    let mut functions = MetaFunctions::default();
    let mut output = [42; 128];
    // SAFETY: The fixture keeps all native inputs and hook globals live.
    unsafe {
        assert_eq!(
            attach::<Admission>(PT_STARTUP, &mut functions, &raw mut globals),
            1
        );
        assert_eq!(
            client_connect::<Admission>(
                ptr::null_mut(),
                c"client".as_ptr(),
                c"loopback".as_ptr(),
                output.as_mut_ptr()
            ),
            1
        );
        assert_eq!(globals.mres, MRES_IGNORED);
        assert_eq!(output, [42; 128]);
        assert!(matches!(
            STATE.get(),
            State::Attached { disabled: true, .. }
        ));
        game_shutdown::<Admission>();
    }
}

#[test]
fn admission_can_borrow_the_owned_connection_snapshot() {
    #[derive(Default)]
    struct Admission;
    impl Plugin for Admission {
        const METADATA: Metadata = METADATA;
        fn client_connect<'a>(
            &'a mut self,
            _: &mut Context<'_>,
            client: &'a ClientConnection,
        ) -> ClientAdmission<'a> {
            ClientAdmission::reject(&client.name).unwrap()
        }
    }
    queried();
    let mut globals = MetaGlobals::default();
    let mut functions = MetaFunctions::default();
    let mut output = [42; 129];
    // SAFETY: All inputs, output storage, and hook globals remain live.
    unsafe {
        assert_eq!(
            attach::<Admission>(PT_STARTUP, &mut functions, &raw mut globals),
            1
        );
        for len in [0, 1, 127] {
            let input = std::ffi::CString::new(vec![0xff; len]).unwrap();
            output.fill(42);
            assert_eq!(
                client_connect::<Admission>(
                    ptr::null_mut(),
                    input.as_ptr(),
                    c"loopback".as_ptr(),
                    output.as_mut_ptr()
                ),
                0
            );
            assert_eq!(globals.mres, MRES_SUPERCEDE);
            assert!(output[..len].iter().all(|&byte| byte as u8 == 0xff));
            assert_eq!(output[len], 0);
            assert!(output[len + 1..].iter().all(|&byte| byte == 42));
        }
        game_shutdown::<Admission>();
    }
}
