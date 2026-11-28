# Model and sound precaching

Opt into `Plugin::PRECACHE` and implement `precache` to register assets before
the GameDLL's worldspawn. The callback runs synchronously at most once per
observed map load; it is skipped if plugin state is already borrowed.

```rust
use metamod_sdk_rs::prelude::*;

const MODEL: &ResourcePath = ResourcePath::new(c"sprites/laserbeam.spr").unwrap();
const SOUND: &ResourcePath = ResourcePath::new(c"common/null.wav").unwrap();

#[derive(Default)]
struct Assets {
    model: Option<ModelIndex>,
    sound: Option<SoundIndex>,
}

#[metamod_plugin(name = "Assets", version = "0.1.0", author = "Developer")]
impl Plugin for Assets {
    const PRECACHE: bool = true;

    fn precache(&mut self, resources: &mut Precache<'_>) {
        self.model = resources.model(MODEL).ok();
        self.sound = resources.sound(SOUND).ok();
    }
}
```

The engine demo uses these assets and exposes `rust_resources` to show the
indices and whether they belong to the active map. Ensure the files exist
in the selected game's search path before loading that example. The SDK
has no dependency on these particular assets or any GameDLL class.

## Names and indices

`ResourcePath::new` is an allocation-free const constructor returning `Option`;
`TryFrom<&CStr>` returns `InvalidResourcePath`. Names contain 1–63 ASCII bytes
using letters, digits, `_`, `-`, `.`, and `/`. Empty, `.` and `..` path
components, backslashes, absolute paths, inline model names, and sentence
names are excluded. Case is preserved. Models and sprites are relative to
the mod's search path; sound names are relative to `sound/`.

Registration requires `&'static ResourcePath`. Some native implementations
retain the submitted pointer, so a temporary `CString` is insufficient.
The SDK neither allocates nor leaks name storage. Dynamic name ownership,
generic downloads, and event-script registration are outside this increment.

`ModelIndex` and `SoundIndex` are separate types. `get()` exposes a positive
native index for diagnostics/raw integration; its value alone has no map
identity. `ctx.is_model_current(index)` and `ctx.is_sound_current(index)`
require the same resource generation and active map. They return false
during loading and after deactivation, shutdown, or a later map load.
Saving an index does not pin a resource or make it valid in client messages.

## Native contracts and failure behavior

The pinned [engine table](../upstream/hlsdk/engine/eiface.h) supplies the
precache callbacks. ReHLDS's [resource implementation](https://github.com/rehlds/ReHLDS/blob/master/rehlds/engine/pr_cmds.cpp)
allows new models and sounds while `ss_loading`; outside that phase, an
unregistered name causes a host error. Its non-`REHLDS_FIXES` branches retain
the caller's name pointer rather than copying it.
[Map loading](https://github.com/rehlds/ReHLDS/blob/master/rehlds/engine/sv_main.cpp)
enters loading before [map entity dispatch](https://github.com/rehlds/ReHLDS/blob/master/rehlds/engine/pr_edict.cpp)
spawns world index zero. This is the native lifecycle contract used here,
not a phase inferred from `globalvars_t` time or from an engine version label.

The SDK identifies worldspawn by comparing its pointer with engine index zero.
It opens no scope for runtime spawns, rejects nested identification, and closes
the scope before activation or after its callback returns. Nested worldspawn,
deactivation or shutdown revokes an interrupted registration. Returned values
are checked again after native calls before an index is published. Submitted
static names remain readable even if the map changes during a native call.
The host must preserve callback code, obey const input, deliver the expected
lifecycle, and handle its own native reentry correctly. Forced unloading is
unsupported.

`PrecacheError` covers an unavailable scope, absent native callback, or a
nonpositive returned index. Path validation does not establish file existence,
asset format validity or native table capacity. Missing/corrupt models and
resource exhaustion can cause fatal native errors; the SDK cannot catch or
translate them into Rust errors. Host callbacks must not unwind or jump
across Rust frames. Use trusted assets and test a deployment before production.

## Verification status

Focused Rust/Miri tests cover static retention, nested spawn, interrupted
activation, shutdown during a call, panic containment, invalid indices,
missing callbacks, and map-scoped identity. Const and compile-fail tests cover
path syntax and rejection of temporary names. The shared x86 loader checks
loading-only calls, ignored spawn results, retained names through shutdown,
and index expiry; layout probes compare callback offsets with pinned headers.

This feature has no live-server evidence yet. Synthetic checks do not extend
the historical core v2 certificate. See the [support matrix](support-matrix.md)
for target status.
