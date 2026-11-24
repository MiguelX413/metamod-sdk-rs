#[test]
fn client_admission_is_synchronous_and_bounds_the_rejection_buffer() {
    #[derive(Default)]
    struct Admission;
    impl Plugin for Admission {
        const METADATA: Metadata = METADATA;
        const CLIENT_CONNECT_PRE: bool = true;

        fn client_connect(
            &mut self,
            _: &mut Context<'_>,
            client: &ClientConnection,
        ) -> ClientAdmission {
            assert_eq!(client.address.as_c_str(), c"loopback");
            match client.name.as_bytes() {
                b"defer" => ClientAdmission::Defer,
                b"panic" => panic!("admission panic"),
                b"exact" => {
                    ClientAdmission::Reject(std::ffi::CString::new(vec![b'x'; 127]).unwrap())
                }
                b"utf8" => ClientAdmission::Reject(
                    std::ffi::CString::new(format!("{}é", "x".repeat(126))).unwrap(),
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
                    ClientAdmission::Reject(std::ffi::CString::new(vec![b'x'; 200]).unwrap())
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
    struct Admission;
    impl Plugin for Admission {
        const METADATA: Metadata = METADATA;

        fn client_connect(
            &mut self,
            _: &mut Context<'_>,
            _: &ClientConnection,
        ) -> ClientAdmission {
            // SAFETY: This mock keeps the attached hook globals live.
            unsafe { game_shutdown::<Self>() };
            assert!(!DROPPED.get());
            ClientAdmission::Reject(c"too late".to_owned())
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

