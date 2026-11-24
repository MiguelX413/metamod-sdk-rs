//! Hook results, guarded plugin borrows, and deferred event delivery.

use super::state::{
    EVENTS, Event, INSTANCE, MAX_EVENTS_PER_DISPATCH, RUNNING, SHUTDOWN, STATE, Services, State,
    contained, make_context, wrong_thread,
};
use crate::engine;
use crate::{Context, HookResult, Plugin};
use metamod_sys::*;
use std::io;
use std::io::Write;
use std::mem::ManuallyDrop;
use std::ptr;

struct ResultWriter(*mut MetaGlobals, MetaResult);

impl Drop for ResultWriter {
    fn drop(&mut self) {
        // SAFETY: The attached server thread owns the hook result, and
        // Metamod keeps the globals live for this entire invocation.
        unsafe { ptr::addr_of_mut!((*self.0).mres).write(self.1) };
    }
}

pub(super) fn hook(callback: impl FnOnce()) {
    let _ = contained(|| {
        if wrong_thread() {
            return;
        }
        if let State::Attached { globals, .. } = STATE.get() {
            let _result = ResultWriter(globals, MRES_IGNORED);
            engine::invalidate_string_read();
            callback();
        }
    });
}

pub(super) fn submit<P: Plugin>(event: Event) {
    let State::Attached {
        disabled: false, ..
    } = STATE.get()
    else {
        return;
    };
    let nested = RUNNING.get();
    if SHUTDOWN.get() || matches!(event, Event::Frame) && nested {
        return;
    }
    let accepted = EVENTS.with(|events| {
        let mut events = events.borrow_mut();
        if !nested {
            events.submitted = 0;
        }
        if events.submitted == MAX_EVENTS_PER_DISPATCH {
            return false;
        }
        events.submitted += 1;
        events.pending.push_back(event);
        true
    });
    if accepted {
        drain::<P>();
    } else {
        disable_callbacks();
        // Metamod and engine logging can dispatch hooks. Use stderr
        // directly, after disabling callbacks and releasing the queue borrow.
        let _ = writeln!(
            io::stderr(),
            "[metamod-sdk-rs] warning: plugin {:?} exceeded its dispatch budget of {MAX_EVENTS_PER_DISPATCH} events; user callbacks disabled",
            P::METADATA.name
        );
    }
}

fn disable_callbacks() {
    if let State::Attached {
        services, globals, ..
    } = STATE.get()
    {
        STATE.set(State::Attached {
            services,
            globals,
            disabled: true,
        });
    }
    EVENTS.with(|events| events.borrow_mut().pending.clear());
}

struct Running;

impl Drop for Running {
    fn drop(&mut self) {
        RUNNING.set(false);
    }
}

fn invoke<P: Plugin, T>(
    services: Services,
    callback: impl FnOnce(&mut P, &mut Context<'_>) -> T,
) -> Option<T> {
    contained(|| {
        INSTANCE.with(|instance| {
            let mut slot = instance.borrow_mut();
            let plugin = slot.as_mut()?.downcast_mut::<P>()?;
            let scope = ();
            let mut context = make_context::<P>(&scope, services);
            Some(callback(plugin, &mut context))
        })
    })
    .flatten()
}

pub(super) fn synchronous<P: Plugin, T: Default>(
    callback: impl FnOnce(&mut P, &mut Context<'_>, *mut MetaGlobals) -> HookResult<T>,
) -> T {
    contained(|| {
        if wrong_thread() {
            return T::default();
        }
        let State::Attached {
            globals,
            services,
            disabled,
        } = STATE.get()
        else {
            return T::default();
        };
        let mut result = ResultWriter(globals, MRES_IGNORED);
        engine::invalidate_string_read();
        if disabled || SHUTDOWN.get() || RUNNING.replace(true) {
            return T::default();
        }
        let running = Running;
        EVENTS.with(|events| events.borrow_mut().submitted = 1);
        let decision = invoke::<P, _>(services, |plugin, context| {
            callback(plugin, context, globals)
        });
        if decision.is_none() {
            disable_callbacks();
        }
        drop(running);
        drain::<P>();
        if SHUTDOWN.get() || matches!(STATE.get(), State::Attached { disabled: true, .. }) {
            return T::default();
        }
        let (status, value) = match decision.unwrap_or_default() {
            HookResult::Ignored => (MRES_IGNORED, T::default()),
            HookResult::Handled => (MRES_HANDLED, T::default()),
            HookResult::Override(value) => (MRES_OVERRIDE, value),
            HookResult::Supercede(value) => (MRES_SUPERCEDE, value),
        };
        result.1 = status;
        value
    })
    .unwrap_or_default()
}

pub(super) fn drain<P: Plugin>() {
    if RUNNING.replace(true) {
        return;
    }
    let _running = Running;
    let State::Attached { services, .. } = STATE.get() else {
        return;
    };

    while !SHUTDOWN.get() {
        let Some(event) = EVENTS.with(|events| events.borrow_mut().pending.pop_front()) else {
            break;
        };
        if matches!(event, Event::Frame) {
            crate::fake_client::poll_removals();
            if SHUTDOWN.get() {
                break;
            }
            if matches!(STATE.get(), State::Attached { disabled: true, .. }) {
                break;
            }
        }
        let result = invoke::<P, _>(services, |plugin, context| match event {
            Event::Frame => plugin.start_frame(context),
            Event::Activate => plugin.server_activate(context),
            Event::Deactivate => plugin.server_deactivate(context),
            Event::Join(player) => plugin.player_join(context, player),
            Event::Disconnect(player) => plugin.player_disconnect(context, player),
            Event::Command(command) => plugin.server_command(context, &command),
            Event::KeyValue(event) => plugin.key_value(context, &event),
        });
        if result.is_none() {
            disable_callbacks();
            break;
        }
    }
    if SHUTDOWN.get()
        && let Some(instance) = INSTANCE.with(|instance| instance.take())
    {
        let mut instance = ManuallyDrop::into_inner(instance);
        if matches!(
            STATE.get(),
            State::Attached {
                disabled: false,
                ..
            }
        ) {
            let _ = contained(|| {
                if let Some(plugin) = instance.downcast_mut::<P>() {
                    plugin.shutdown();
                }
            });
        }
        let _ = contained(|| drop(instance));
    }
}
