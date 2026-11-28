#[test]
fn entity_appearance_owns_fields_and_rejects_interrupted_native_strings() {
    #[derive(Clone, Copy)]
    enum Mode {
        Normal,
        Unterminated,
        Null,
        MaximumLength,
        Empty,
        NestedRead,
        ReuseDuringString,
        FreeDuringString,
        RetireMap,
        Shutdown,
        ReuseAfterCopy,
        OverwriteAfterCopy,
    }
    thread_local! {
        static SOURCE: Cell<*mut edict_t> = const { Cell::new(ptr::null_mut()) };
        static BUFFER: Cell<*mut u8> = const { Cell::new(ptr::null_mut()) };
        static MODE: Cell<Mode> = const { Cell::new(Mode::Normal) };
        static AFTER_COPY: Cell<Mode> = const { Cell::new(Mode::Normal) };
        static STRING_CALLS: Cell<usize> = const { Cell::new(0) };
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
    unsafe extern "C" fn entity(index: c_int) -> *mut edict_t {
        if index != 2 {
            return ptr::null_mut();
        }
        // SAFETY: The mock owns the current-map source and buffer allocations.
        match AFTER_COPY.replace(Mode::Normal) {
            Mode::ReuseAfterCopy => unsafe { (*SOURCE.get()).serialnumber += 1 },
            Mode::OverwriteAfterCopy => unsafe {
                ptr::copy_nonoverlapping(c"overwritten".as_ptr().cast(), BUFFER.get(), 12)
            },
            _ => {}
        }
        SOURCE.get()
    }
    unsafe extern "C" fn string(index: c_int) -> *const c_char {
        assert_eq!(index, -17, "preserve signed native string offsets");
        STRING_CALLS.update(|calls| calls + 1);
        // SAFETY: These paths use the mock's live source and 4,096-byte buffer.
        match MODE.get() {
            Mode::Unterminated => {
                unsafe { BUFFER.get().write_bytes(b'x', 4096) };
                return BUFFER.get().cast();
            }
            Mode::Null => return ptr::null(),
            Mode::MaximumLength => {
                unsafe {
                    BUFFER.get().write_bytes(b'x', 4095);
                    BUFFER.get().add(4095).write(0);
                }
                return BUFFER.get().cast();
            }
            Mode::Empty => return c"".as_ptr(),
            Mode::NestedRead => {
                let scope = ();
                let ctx = context(&scope);
                let handle = ctx.entity_at(2).unwrap().unwrap();
                assert!(ctx.entity_appearance(handle).is_none());
                return ptr::dangling();
            }
            Mode::ReuseDuringString => {
                unsafe { (*SOURCE.get()).serialnumber += 1 };
                return ptr::dangling();
            }
            Mode::FreeDuringString => {
                unsafe { (*SOURCE.get()).free = 1 };
                return ptr::dangling();
            }
            Mode::RetireMap => {
                crate::trace::invalidate_map();
                // Retire before freeing. Neither the edict nor result may be read.
                unsafe { drop(Box::from_raw(SOURCE.replace(ptr::null_mut()))) };
                return ptr::dangling();
            }
            Mode::Shutdown => {
                SHUTDOWN.set(true);
                return ptr::dangling();
            }
            mode @ (Mode::ReuseAfterCopy | Mode::OverwriteAfterCopy) => AFTER_COPY.set(mode),
            _ => {}
        }
        let bytes = c"models/\xff.mdl".to_bytes_with_nul();
        unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), BUFFER.get(), bytes.len()) };
        BUFFER.get().cast()
    }
    fn source(serial: i32) -> *mut edict_t {
        let mut source = edict_t {
            serialnumber: serial,
            ..Default::default()
        };
        source.v.model = (-17_i32) as _;
        source.v.modelindex = -23;
        source.v.skin = -1;
        source.v.body = 3;
        source.v.effects = 32;
        source.v.sequence = 9;
        source.v.gaitsequence = 4;
        source.v.frame = 12.5;
        source.v.animtime = -2.0;
        source.v.framerate = f32::NEG_INFINITY;
        source.v.controller = [0, 1, 254, 255];
        source.v.blending = [17, 255];
        source.v.scale = f32::NAN;
        source.v.rendermode = 999;
        source.v.renderamt = -128.5;
        source.v.rendercolor.x = 255.5;
        source.v.rendercolor.y = -1.0;
        source.v.rendercolor.z = f32::INFINITY;
        source.v.renderfx = -9;
        source.v.colormap = 0x1234;
        Box::into_raw(Box::new(source))
    }
    queried();
    SOURCE.set(source(41));
    let mut buffer = [0; 4096];
    BUFFER.set(buffer.as_mut_ptr());
    ENGINE.set(Engine {
        entity: Some(entity),
        string_from_index: Some(string),
        ..Default::default()
    });
    let mut globals = EngineGlobals {
        maxEntities: 8,
        ..Default::default()
    };
    // SAFETY: Initialized mock globals remain live until explicitly cleared.
    unsafe { crate::trace::set_globals(&raw mut globals) };
    crate::trace::activate_map();
    let scope = ();
    let ctx = context(&scope);
    let handle = ctx.entity_at(2).unwrap().unwrap();
    let snapshot = ctx.entity_appearance(handle).unwrap();
    assert_eq!(snapshot.entity, handle);
    assert_eq!(snapshot.model_name.as_deref(), Some(c"models/\xff.mdl"));
    assert_eq!(
        (
            snapshot.model_index,
            snapshot.skin,
            snapshot.body,
            snapshot.effects
        ),
        (-23, -1, 3, 32)
    );
    assert_eq!((snapshot.sequence, snapshot.gait_sequence), (9, 4));
    assert_eq!((snapshot.frame, snapshot.animation_time), (12.5, -2.0));
    assert_eq!(snapshot.frame_rate, f32::NEG_INFINITY);
    assert!(snapshot.scale.is_nan());
    assert_eq!(snapshot.controllers, [0, 1, 254, 255]);
    assert_eq!(snapshot.blending, [17, 255]);
    assert_eq!(
        (
            snapshot.render_mode,
            snapshot.render_amount,
            snapshot.render_fx,
            snapshot.colormap
        ),
        (999, -128.5, -9, 0x1234)
    );
    assert_eq!(snapshot.render_color, [255.5, -1.0, f32::INFINITY]);
    for mode in [
        Mode::Unterminated,
        Mode::Null,
        Mode::MaximumLength,
        Mode::Empty,
    ] {
        MODE.set(mode);
        let current = ctx.entity_appearance(handle).unwrap();
        assert_eq!(
            current.model_name.map(|name| name.to_bytes().len()),
            match mode {
                Mode::MaximumLength => Some(4095),
                Mode::Empty => Some(0),
                _ => None,
            }
        );
    }
    MODE.set(Mode::NestedRead);
    assert!(ctx.entity_appearance(handle).is_none());
    MODE.set(Mode::ReuseDuringString);
    assert!(ctx.entity_appearance(handle).is_none());
    assert!(!ctx.is_entity_current(handle));
    let handle = ctx.entity_at(2).unwrap().unwrap();
    MODE.set(Mode::ReuseAfterCopy);
    assert!(ctx.entity_appearance(handle).is_none());
    let handle = ctx.entity_at(2).unwrap().unwrap();
    MODE.set(Mode::OverwriteAfterCopy);
    assert_eq!(
        ctx.entity_appearance(handle).unwrap().model_name.as_deref(),
        Some(c"models/\xff.mdl")
    );
    // A later index lookup overwrote the engine buffer, not the owned result.
    assert_eq!(snapshot.model_name.as_deref(), Some(c"models/\xff.mdl"));
    MODE.set(Mode::FreeDuringString);
    assert!(ctx.entity_appearance(handle).is_none());
    // SAFETY: The mock source slot is still allocated on this map.
    unsafe {
        (*SOURCE.get()).free = 0;
        (*SOURCE.get()).v.model = 0;
    }
    let reads = STRING_CALLS.get();
    assert!(ctx.entity_appearance(handle).unwrap().model_name.is_none());
    assert_eq!(STRING_CALLS.get(), reads);
    // SAFETY: The same mock slot remains allocated.
    unsafe {
        (*SOURCE.get()).v.model = (-17_i32) as _;
    }
    ENGINE.update(|engine| Engine {
        string_from_index: None,
        ..engine
    });
    assert!(ctx.entity_appearance(handle).unwrap().model_name.is_none());
    ENGINE.update(|engine| Engine {
        string_from_index: Some(string),
        ..engine
    });
    MODE.set(Mode::Shutdown);
    assert!(ctx.entity_appearance(handle).is_none());
    SHUTDOWN.set(false);
    // Save the serial before the next getter retires and frees the allocation.
    // SAFETY: No retirement has occurred yet; SOURCE still owns this slot.
    let serial = unsafe { (*SOURCE.get()).serialnumber };
    MODE.set(Mode::RetireMap);
    assert!(ctx.entity_appearance(handle).is_none());
    SOURCE.set(source(serial));
    crate::trace::activate_map();
    assert!(ctx.entity_appearance(handle).is_none());
    assert_eq!(snapshot.model_name.as_deref(), Some(c"models/\xff.mdl"));
    // SAFETY: Retire the replacement map before releasing its mock source.
    crate::trace::invalidate_map();
    unsafe {
        drop(Box::from_raw(SOURCE.replace(ptr::null_mut())));
        crate::trace::set_globals(ptr::null_mut());
    }
    ENGINE.set(Engine::default());
    BUFFER.set(ptr::null_mut());
}
