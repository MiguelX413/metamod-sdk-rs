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

The Linux observations below cover three identified deployments. Neither those
runs nor synthetic checks extend the historical core v2 certificate. Win32
and other GameDLLs remain unverified for precaching.

## Live Linux results

The [server-only probe](../examples/engine-demo/examples/precache_lifecycle.rs)
passed on fresh isolated copies with stock Ricochet, using classic Metamod
1.21.1-am or Metamod-R 1.3.0.149 as listed below. No clients connected.
All servers exited zero, and no test servers remained running.
The original saved installations and previous evidence were untouched.

| Engine and manager | `rc_arena` model/sound | `rc_deathmatch` model/sound | Old typed indices |
| --- | --- | --- | --- |
| Valve HLDS build 8684 + classic Metamod 1.21.1-am | 28 / 1 | 36 / 1 | Rejected during deactivation and on the next map |
| ReHLDS 3.15.0.896-dev (`1963001`) + classic Metamod 1.21.1-am | 2 / 1 | 2 / 1 | Rejected during deactivation and on the next map |
| ReHLDS 3.15.0.896-dev (`1963001`) + Metamod-R 1.3.0.149 (`603a257`) | 2 / 1 | 2 / 1 | Rejected during deactivation and on the next map |

On each map, the probe registered both assets twice and checked that duplicate
requests returned equal indices. Activation and a later console command
confirmed that both indices belonged to the active map. Deactivation rejected
them, and the next activation rejected the saved old handles while accepting
the new ones. ReHLDS reused both numeric indices; the sound index was also
reused on Valve HLDS. Numeric equality did not revive the old handles.

The [transcript checker](../verify/precaching/check-log.py) requires the complete,
ordered registration/activation/status/deactivation sequence, one stale-index
record, and correlated positive indices. The runner separately requires exit
zero. It rejects missing, duplicate, failed and out-of-order probe records.

These runs were uninstrumented. They establish server-side registration and
SDK lifecycle behavior, not client downloads, model rendering, sound playback,
or universal memory safety. Missing/corrupt assets, resource exhaustion and
forced native reentry were not exercised on live servers.

### Evidence and provenance

Evidence is on the audit host under `~/precache-lifecycle.f3pEcA/`.
`run/` retains both isolated servers, transcripts, and exit records;
`summary.json` records checked results and component hashes. The build manifest
is `precache-live-build.json`.

SDK sources matched `c87d4da6062e1fcf29fa28f4f3ae68d18f721c5b` without changes
to the SDK/raw/macro crates. The new probe was an uncommitted development
example, not a clean build of that commit. Its build revision remains unknown;
the manifest records source/artifact hashes, dirty state, Rust 1.100.1 and
target `i686-unknown-linux-gnu`. No compiler was needed on the audit host.

| Artifact | SHA-256 |
| --- | --- |
| Rust probe | `f3be33967d622ea0dd1037f6631f64812cf307fe9d7ac35cb1c48f174c308010` |
| Shared launcher | `f8a46f49c43ceb4a744bc4a415f4360f7fb93ac112a76616f84dd52c26168e18` |
| Valve engine module | `52e7ed23455eeb49a1561114fcde230b738cf94b40e2d77953555fb32ed07ba8` |
| ReHLDS engine module | `5bb5d862b7e65ced8c9cc5a2159d52d5522e3eb784ba0eb88e6624a410566e99` |
| Classic Metamod | `6956fb8be550189c827f14b919d998551c81b34e75becbb705c0b085d5390a20` |
| Stock Ricochet GameDLL | `57ea9f913b543d9fdfedcab43edfef8fcf5754fc9fada2bee6cac620eb4ce3e4` |
| Valve transcript | `3fc202c2dc5cecfc76fd32866e44723a723830378fd967e17f2ba104594ae0c9` |
| ReHLDS transcript | `fd285027d704507d91da1c0d5e9ca142012dac3098ecaa1e7d76c631d1468270` |
| Build manifest | `3d4240fbf291575006a4b755732771153daac0a924e24f5ad123de5ed36f6f10` |
| Executed runner | `0d239894740e0c3dc77dddd1ddecfe926ba0c71f586a3b2a2ceafe070eb8bd64` |
| Executed transcript checker | `542d7f70694044eeb87492673a808ad5e64c4d31ff6eea04048c5d8eb54f5c0c` |

### Metamod-R follow-up

The follow-up reused the exact same probe binary on a fresh ReHLDS copy
with Metamod-R 1.3.0.149. Registration and duplicate checks passed on both
maps; old handles stayed stale when model index `2` and sound index `1`
were reused. The complete transcript and zero exit record passed independent
validation. No test server remained running.

Evidence is under `~/precache-lifecycle-mmr.Kf6FnP/`, with the isolated server,
transcript and exit record in `run/`. `summary.json` records checked results
and component hashes. The original build manifest was retained unchanged:
no rebuild or new clean-build provenance is claimed. The runner gained a
ReHLDS-only Metamod-R mode; its hash is separate from the original runner.
The launcher, engine, GameDLL and probe hashes match the earlier ReHLDS run.

| Artifact | SHA-256 |
| --- | --- |
| Metamod-R module | `8bed09c362f6d9019ce7d88b9a19d1ad08eca26bf46c0ed32604fbea2d7ba7a9` |
| ReHLDS/Metamod-R transcript | `3ea96090478a69030ae613b5509f6317f5865a2b2a6501dac3dc73d317614950` |
| Executed follow-up runner | `c51a8d775f20386555fced92c04e98ac222828bd321917c307a46c15b79a8fe3` |

### Reproduce the live test

Build on the development machine with the usual i686 prerequisites:

```bash
cargo build --locked --release --target i686-unknown-linux-gnu \
  -p engine-demo --example precache_lifecycle
sha256sum target/i686-unknown-linux-gnu/release/examples/libprecache_lifecycle.so
```

Record the actual build/source provenance alongside the binary. Transfer it
and the runner/checker, preserving their `verify/fake-client/` and
`verify/precaching/` relative directories. On the provisioned test host:

```bash
bash verify/fake-client/run-linux.sh \
  "$HOME/ricochet-audit-lab/runs/20261122T115759Z-6890/installs" \
  ./libprecache_lifecycle.so "$HOME/precache-new-run" precache
```

For only ReHLDS with Metamod-R, use a separate output directory and the same
prebuilt binary:

```bash
bash verify/fake-client/run-linux.sh \
  "$HOME/ricochet-audit-lab/runs/20261122T115759Z-6890/installs" \
  ./libprecache_lifecycle.so "$HOME/precache-mmr-new-run" precache-metamod-r
```

The output directory must not exist. Reserve loopback port 27057 and space
for two full copies (one with `precache-metamod-r`). Both precaching modes
load this Rust probe with the stock GameDLL through the shared launcher.
Do not connect clients during this controlled test. To recheck a preserved
transcript without a server:

```bash
python3 verify/precaching/check-log.py /path/to/linux-hlds.log
```
