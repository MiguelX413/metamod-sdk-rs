# SDK reference

[Project overview](../README.md)

- [Write a Metamod plugin](#write-a-metamod-plugin)
- [Engine services](#engine-services)
- [Build and install hello-world](#build-and-install-hello-world)
- [Architecture and safety](#architecture-and-safety)
- [Native memory and registration contracts](#native-memory-and-registration-contracts)
- [Typed hooks and coverage](#typed-hooks-and-coverage)
- [Verify](#verify)
- [Licensing and references](#licensing-and-references)
- [GoldSrc runtime audit](#goldsrc-runtime-audit)

## Write a Metamod plugin

Set your library's crate type to `cdylib` and add `metamod-sdk-rs` as a
dependency. The default `macros` feature provides `#[metamod_plugin]`, which
fills in metadata and exports the Metamod entry points.

```toml
[dependencies]
metamod-sdk-rs = { git = "https://github.com/MiguelX413/metamod-sdk-rs" }
```

```rust
use metamod_sdk_rs::prelude::*;

#[derive(Default)]
struct MyPlugin {
    greeted: bool,
}

#[metamod_plugin(cargo, name = "My Plugin")]
impl Plugin for MyPlugin {
    fn start_frame(&mut self, ctx: &mut Context<'_>) {
        if !self.greeted {
            self.greeted = true;
            ctx.server().print(c"Hello from Rust!\n");
        }
    }
}
```

Use `#[metamod_plugin(cargo)]` to inherit the Cargo package name, version,
authors, and repository URL, including values inherited from the workspace.
With that opt-in, set a custom display name and log tag in the plugin's
`Cargo.toml`:

```toml
[package.metadata.metamod]
name = "Rust Hello World"
log_tag = "RUSTHELLO"
```

Explicit attribute fields take precedence over the metadata table and Cargo
package defaults. Without `cargo`, supply `name`, `version`,
and `author`. Without `cargo` or an explicit `url`, the URL is empty.
The `date` defaults to empty and `log_tag` defaults to the display name.
Set `default-features = false` on the dependency to omit procedural macros.
For manual registration, declare `const METADATA: Metadata` in your `impl Plugin` using static
C strings, then call `export_plugin!(MyPlugin)` once in the library. Both forms
use the same dispatch. To migrate a stateless plugin, implement `Default`
and add `&mut self` and `&mut Context<'_>` to its callbacks. Existing
`Context::log` and `Context::print_console` methods still accept C strings.

| Plugin method | Timing |
| --- | --- |
| `initialize` | Once per startup attach attempt, after construction. Return `false` to decline. |
| `start_frame` | Before the game DLL's frame callback. |
| `server_command` | When GoldSrc invokes a command registered by this plugin. |
| `server_activate` | After the game DLL activates the map. |
| `server_deactivate` | Before the game DLL deactivates the map. |
| `player_join` | After the game DLL puts a player into the server. |
| `player_disconnect` | When the SDK observes a disconnect and invalidates the session, normally before the game DLL callback. |
| `shutdown` | If Metamod forwards `GameShutdown`, once before the SDK drops the plugin instance. Engine access is unavailable. |

`PlayerSlot` is a reusable one-based GoldSrc index. `PlayerSlot::new(index)`
takes an `i32` and rejects zero and negative values. Both `PlayerSlot::get()`
and `PlayerId::get()` return the same one-based `i32` index.
`PlayerId` identifies an observed session in that slot for the current map.
Keep the full ID as the key for per-player state. Use
`ctx.is_player_current(id)` to validate a saved ID and `ctx.player_at(slot)`
to look up the session currently tracked in a slot. A disconnect, slot replacement,
or map boundary invalidates old IDs immediately. Tracking depends on the
host delivering its normal join, disconnect, and map callbacks on the
server thread. It covers players whose joins the plugin observed.

Parse UTF-8 input with `text.parse::<PlayerSlot>()` or
`text.parse::<PlayerUserId>()`. Both use standard Rust `i32` syntax.
`ParsePlayerNumberError::Integer` preserves the underlying integer error,
including overflow. `Invalid` reports a parsed zero or negative value.
For native command arguments, call `arg.to_str()` before parsing.

Use the lifecycle hooks to clear map-specific state and
`ctx.server().print(&CStr)` for console messages within a callback. Use a
`c"..."` literal or construct a `CString` for dynamic text. The SDK sends
the original bytes. Include a newline for line output. For Metamod's
line-oriented utilities, use `ctx.metamod().print_console_trunc(c"Ready")`
or `ctx.metamod().log_trunc(c"Ready")`. Both accept any `&CStr` and truncate
at 1,021 payload bytes, excluding the NUL terminator. Truncation can split
a UTF-8 sequence.

If you need to reject oversized messages, convert with
`let message: &MetamodMessage = c"Ready".try_into()?;` and pass the validated
borrow to `ctx.metamod().print_console(message)` or `ctx.metamod().log(message)`.
The existing `Context::print_console` and `Context::log` methods forward to
those truncating methods.

## Engine services

Use `ctx.server()` to register, read, and set CVars. For examples, see
[engine-demo](../examples/engine-demo/src/lib.rs), which declares its metadata
explicitly in the attribute. At the server console with that demo installed,
`rust_hello` prints the current `rust_hello_message` value and
`rust_hello "New message"` updates it. `Server::cvar` returns an owned
`CString`, with `Ok(None)` for an absent variable. Values supplied by Rust
must fit in 1,023 bytes excluding the terminator. Construct a borrowed
`CvarValue` with `CvarValue::new(value)` before calling
`set_cvar` or `register_cvar`. Validation needs no allocation and preserves
the original bytes. Registration copies the value into retained storage.
Use `ConsoleName::new(raw_name)` to borrow a validated lowercase name
without allocating. Use `OwnedConsoleName::try_from(raw_name)` to own an
already-lowercase name from a `&CStr` or `CString`. Use
`OwnedConsoleName::normalize` to validate and lowercase a `CString` in place.
Command registration and CVar operations borrow the validated name, so an
owned name can be reused for subsequent reads and writes.
Values and console output can contain non-UTF-8 bytes.

For literals, `console_name!` and `cvar_value!` validate at compile time:

```rust
use metamod_sdk_rs::prelude::*;

static NAME: &ConsoleName = console_name!(c"MY_PLUGIN_SETTING");
const DEFAULT: &CvarValue = cvar_value!(c"1");
```

Both macros return static references. The name macro stores normalized
lowercase bytes without allocating.

`Box<ConsoleName>` and `Box<CvarValue>` accept fallible conversions from
`&CStr`, `CString`, and `Box<CStr>`. All these conversions preserve the
input bytes. Converting from `Box<CStr>` also reuses the allocation.
Use `ConsoleName::normalize_boxed` to lowercase a boxed name in place.
Both boxes also support cloning and
infallible conversion from an already validated reference.

Register names from an attached callback such as `start_frame` or
`server_activate`. Use a plugin-specific prefix and register each name once
per process. Names are case-insensitive and accept up to 63 ASCII letters,
digits, underscores, or hyphens. The SDK prevents duplicate command names
within a plugin and checks for existing CVars. GoldSrc's command
registration returns no status and exposes neither a command lookup nor
portable deregistration. Choose names that do not collide with engine or
other plugin commands. Commands and CVars retain registration storage for
the process lifetime. Registration is unavailable in `initialize`, where
the plugin can still decline attachment. [Metamod registration code](https://github.com/alliedmodders/metamod-hl1/blob/18a10db686702e8ae9e0fcb5b5febf8881dc9c2d/metamod/reg_support.cpp) · [ReHLDS CVar implementation](https://github.com/rehlds/ReHLDS/blob/master/rehlds/engine/cvar.cpp)

`Command::name()` returns a borrowed, normalized `&ConsoleName`. Use
`command.name().as_c_str()` to compare it with a C-string literal.
`arg(0)` is the first
argument after the name. Arguments are copied as C strings before delivery,
so deferred commands do not depend on the engine's transient argument buffer.
Absent, oversized, or observably reentrant snapshots are skipped. The limit
is 128 tokens including the name and 4,096 bytes including terminators.
Native command callbacks share the dispatcher and its panic and event-budget
rules. The portable API does not expose the command sender or authenticate
the invocation.

`ctx.player_info(id)` returns an owned snapshot for a currently observed
session. Its `Option<PlayerUserId>` contains the positive server-assigned
GoldSrc user ID when available. `PlayerUserId` is distinct from a reusable
`PlayerSlot` and a map-scoped `PlayerId`. Use `PlayerUserId::try_from(raw)`
to validate an integer and `i32::from(user_id)` to recover it. The snapshot also contains the
authentication string when available. It returns `None` for a stale ID or a missing entity lookup and
revalidates the session around engine calls. An authentication string can
contain an engine marker such as a pending or bot value. Treat it as engine
information, with session validity checked separately when using a saved
snapshot.

### GameDLL keyvalue events

Set `Plugin::KEYVALUE_PRE` to `true` and implement `Plugin::key_value` to
observe GameDLL keyvalue inputs, including those consumed before map
activation. `KeyValueEvent` owns the optional classname and the key and
value as C strings. Null required inputs and payloads exceeding a combined
4,096-byte read budget are skipped. The budget includes terminators.

The hook leaves the native data and GameDLL result unchanged. During
reentry, the SDK copies the inputs before returning to the engine and
delivers the observation through the deferred queue. No edict pointer or
map-bound handle is carried by the event, so it can represent inputs that
arrive before the map is active. The engine demo logs `classname` keys.

### Tracing and entity handles

Create a segment with `TraceSegment::new(start, end)`. The const constructor
rejects NaN and infinities in either world-space `[f32; 3]` endpoint. Reuse
the validated segment with `ctx.server().trace_line(segment, options)` or
`trace_hull(segment, Hull::Human, options)` on an active map.
Results own the fraction, end
position, solid flags, optional plane, hit group, and optional entity handle.
The engine demo's `rust_trace` command shows both trace kinds.

A trace hit's `EntityHandle` records the map epoch, engine index, and native
edict serial. Validate saved handles with `ctx.is_entity_current(handle)`.
Passing a stale handle as `TraceOptions::skip` fails before tracing. A map
transition during a trace discards its output. Handles depend on the engine
maintaining edict serials and Metamod forwarding map lifecycle hooks. A
collision can have no usable handle, so inspect the fraction separately.

### Entity discovery

On an active map, use `ctx.entity_at(index)` for a single lookup or
`ctx.entities()` for an index-ordered scan of the engine-visible edict
table. The SDK captures `globalvars_t.maxEntities` at map activation and
accepts capacities from 1 through 65,536. An absent or invalid capacity
disables index lookup, handle resolution, and discovery for that map.
Traces can still report geometry without resolving a hit handle.
Scanning that capacity can discover allocations created later in the map. Vacant, out-of-range, and
engine-filtered indices have no handle. A map change during traversal
returns an error. Discovery requires the engine globals and edict lookup
callback. A scan is a sequence of lookups, so individual handles can expire
before you use them.

`ctx.entity_spatial(handle)` revalidates the generation and copies origin,
angles, velocity, and absolute world-space bounds. The result owns its
data. `ctx.entity_info(handle)` classifies the entity as world, client, fake
client, monster, or other using the world index and GoldSrc flags. It copies
the common retained string keyvalues `classname`, `globalname`, `target`,
`targetname`, `model`, `netname`, and `message` into owned C strings. Zero
indices and null strings are absent. GameDLL-private keys and map keys the
GameDLL consumed during loading are outside this snapshot.

Use `rust_entities` or `rust_entities 7` in the engine demo to show the
classification, classname, targetname, and origin. The SDK copies strings
before the next engine call and discards the snapshot if the map or entity
generation changes during lookup. Owned snapshots remain readable after the
handle expires.

## Build and install hello-world

### Linux i686

Initialize the pinned Metamod and HLSDK submodules before building. Use Rust
stable 1.88 or newer, Clang and host libclang, and C/C++ development
headers for the target. For Linux i686, also install 32-bit linker and libc
development files. On Ubuntu, the CI workflow installs `clang`,
`libclang-dev`, and `g++-multilib`. Set `LIBCLANG_PATH` if bindgen cannot
find your host libclang.

```sh
git submodule update --init
rustup target add i686-unknown-linux-gnu
(cd examples/hello-world && cargo build --locked --release)
```

Each example and the audit plugin includes its own `.cargo/config.toml`
that defaults to Linux i686. Run Cargo from the plugin directory to use
that setting. For the live audit runner, use
`(cd metamod-audit && cargo build --locked --release)`. In this workspace,
the outputs go to `target/i686-unknown-linux-gnu/release/`.
When copying an example as a template, include its `.cargo` directory.
Cargo commands run from the workspace root use the host target by default.
An explicit `--target` overrides the plugin's default.

### Windows x86

Install the Visual Studio C++ build tools and Windows SDK, plus LLVM with
host libclang. Use an **x86 Native Tools Command Prompt**. Set
`LIBCLANG_PATH` to the LLVM `bin` directory if bindgen cannot locate libclang.

```bat
git submodule update --init
rustup target add i686-pc-windows-msvc
cargo build --locked --release --target i686-pc-windows-msvc -p hello-world
```

### Install the plugin

In an **isolated Linux server** where Metamod is already installed, copy
`target/i686-unknown-linux-gnu/release/libhello_world.so` to
`<game>/addons/rust-hello/libhello_world.so`. Add this line to the server's
`<game>/addons/metamod/plugins.ini`:

```text
linux addons/rust-hello/libhello_world.so
```

For a **Windows server**, copy
`target/i686-pc-windows-msvc/release/hello_world.dll` to
`<game>/addons/rust-hello/hello_world.dll` and add this line instead:

```text
win32 addons/rust-hello/hello_world.dll
```

Start the server with a valid map. Run `meta list` in the server console and
look for `Rust Hello World` with status `RUN`. Look for `Hello from Rust
Metamod` at attach and `Rust StartFrame hook ran` on the first frame in the
server console. The plugin returns `MRES_IGNORED` each frame, leaving game-DLL
dispatch to Metamod. To remove it, stop the server and remove the configuration
line and shared library.

### Engine demo

On Linux, run `(cd examples/engine-demo && cargo build --locked --release)`.
On Windows, build
with `-p engine-demo`. Use `libengine_demo.so` on Linux or
`engine_demo.dll` on Windows in the corresponding `plugins.ini` entry.
`meta list` shows `Rust Engine Demo`, and the console controls become available
on the first frame. The hello-world example uses Cargo metadata, while
engine-demo shows explicit attribute arguments.

For a live smoke test, use a disposable server on each platform and record
the engine and Metamod versions. Start with the demo configured at startup,
confirm `meta list` reports it running, then run `rust_hello`,
`rust_hello "test value"`, `rust_entities 0`, and `rust_trace` from the
server console. Connect, disconnect, and reconnect a client. Check that the
demo reports the new join and that the server continues serving frames.
Use `changelevel` to load another installed map and repeat the commands.
Stop the process normally with `quit`. These steps exercise a real host's
loader, callbacks, and native buffers. They cannot establish shutdown
delivery or pointer validity in every host and GameDLL combination.

### Client admission

Set `Plugin::CLIENT_CONNECT_PRE` to `true` and implement
`Plugin::client_connect` to inspect the proposed client's name and address.
These are owned native strings. The hook runs before the GameDLL processes
the connection, so it has no established `PlayerId`.

`ClientAdmission::Defer` leaves the decision to the GameDLL and other
plugins. `ClientAdmission::Reject(c"Reason".to_owned())` returns false
with `MRES_SUPERCEDE` and supplies a NUL-terminated reason. GoldSrc's
128-byte output buffer limits the reason to 127 bytes, so truncation can
split a UTF-8 character. Nested admission, missing or unterminated inputs,
and plugin panics
fall back to `MRES_IGNORED`. The host must supply readable inputs and keep
the output buffer writable until the hook returns.

The engine demo passed live admission tests on Linux i686 with both Valve
HLDS and ReHLDS. Win32 live testing is pending. [Live Ricochet results](runtime-audit.md#live-ricochet-client-admission-results)

#### Test both outcomes with the engine demo

Build and install the engine demo as described above. Restart a disposable
server with a valid map and check that `meta list` shows `Rust Engine Demo`
running. Use a real GoldSrc client for the same game and substitute your
server's reachable address and port below. The demo compares the supplied
name byte for byte.

1. While disconnected, run `name "RustAdmissionAllow"` and then
   `connect 127.0.0.1:27015` in the **client console**. The server should
   log `Rust admission demo: deferred`. With ordinary admission settings,
   the client joins and the demo prints its player-slot greeting. The
   GameDLL and any other plugins still decide whether to admit the client.
2. Run `disconnect` in the client console, set the name to
   `RustAdmissionReject`, and repeat the `connect` command. The server
   should log `Rust admission demo: rejected`, and the client should
   receive `Rust admission demo rejected this name.` The attempt should
   produce no player-slot greeting.
3. Restore your original name and reconnect to confirm the server still
   accepts ordinary connections.

### Constructing a user-message payload

`UserMessagePayload` assembles GoldSrc integer and C-string fields into
an owned, allocation-free buffer. Its 192-byte limit includes string
terminators and excludes wire framing. A field that exceeds the remaining
space leaves the payload intact. Use `validate_fixed_size` to check an
expected payload length before handing it to a protocol-specific layer.
Construction makes no engine calls. In an active callback,
`ctx.metamod().user_message(name)` resolves a registration by its native
C-string name, and `RegisteredUserMessage::validate_payload` checks its
declared size. `ctx.message_recipient(player_id)` checks a current tracked
session and its live edict. These observations can expire during later
native calls. Recipient validation checks identity and leaves protocol
readiness and eligibility for future delivery checks. Native sending
remains separate.
[Messaging milestones](api-coverage.md#validated-user-messages)

## Architecture and safety

- `metamod-sys` uses libclang to parse a small C++ wrapper around the pinned
  upstream headers for Cargo's target. Bindgen generates the allowlisted
  interface and its dependent types in Cargo's `OUT_DIR`. The raw module
  exposes the upstream field names, integer enums, and nullable C function
  pointers. The export macro checks its entry points against the generated
  function pointer types at compile time.
- `metamod-sdk-rs` constructs one `Default` state value per startup attach attempt
  and mutably borrows it for each callback. State survives map changes.
  See [hello-world](../examples/hello-world/src/lib.rs) for registration and
  callback examples.
- The first valid `GiveFnptrsToDll` handoff binds the SDK to that server
  thread. The loader then calls query, attach, and hooks on the same thread. It keeps Metamod globals live during
  hooks and engine globals live until shutdown. The SDK copies the function
  pointers it uses and reads the edict capacity at post-activation. It copies
  native spatial fields after revalidating the handle. Metadata and its strings have module
  lifetime. Scoped contexts and server handles cannot cross threads or
  outlive their callbacks. An off-thread hook skips user code and leaves
  server-thread globals alone.
- Query accepts interface major 5 with minor 13 or newer. Both pre- and
  post-`GetEntityAPI2` factories negotiate game-DLL interface 140. The optional
  new-DLL table negotiates its own version and observes `GameShutdown`.
  Observational hooks and skipped, disabled, or panicking synchronous hooks
  set `MRES_IGNORED`. Successful synchronous hooks publish their typed result.
- Nested frame callbacks are skipped. Nested map, player, and command
  notifications are queued in arrival order and delivered after the active Rust callback
  releases its mutable state borrow. Only owned event data enters the queue.
  The SDK updates session tracking immediately. A deferred callback can
  therefore observe an event for a session or map that has already ended.
  If another player or map event interrupts engine slot resolution, tracking
  is cleared and the ambiguous lookup is discarded.
- A dispatch accepts up to 4,096 events, counting its initial callback and
  all events enqueued during delivery. A submission beyond that budget
  discards queued notifications and disables user callbacks for the
  attachment. A one-time warning goes to process stderr and identifies the
  plugin and the exceeded budget. Engine bookkeeping and hook results
  continue, and an observed
  shutdown still drops the instance. This bounds both queue growth and
  callback feedback chains. A callback that never returns remains outside
  the budget's control.
- Construction or initialization panic fails attach. A rejected or failed
  instance is dropped with separate panic containment. A hook panic discards
  pending notifications and disables user hooks while SDK bookkeeping and
  hook results continue. Panic payloads are dropped under a separate unwind
  guard. If a payload destructor panics, its secondary payload is retained
  to stop repeated destructor panics. Use the default unwind strategy. Aborting panics,
  double panics, panics in panic hooks, and foreign faults can terminate the
  server.
- Plugins load at startup and advertise `PT_NEVER`, refusing detach. Use
  `server_deactivate` for map-specific cleanup. If Metamod forwards
  `GameShutdown`, the SDK revokes engine access, discards pending notifications,
  and runs `shutdown` on a healthy instance once the current Rust callback
  returns. It then attempts to drop the instance even if a hook or cleanup
  panicked. The host can omit this signal, in which case the instance stays
  resident without an implicit thread-exit destructor. Forced unload and
  engine restart in the same attachment are unsupported.

Forced unload is a host operation. Classic Metamod and Metamod-R can
override both `PT_NEVER` and a failed `Meta_Detach`, then close the module.
Their unload paths can also override detach refusal for a reload that
reaches that path. Native registrations may still refer to plugin code.
In-flight callbacks or TLS cleanup can also require code or data from the
module. Stop the server process before replacing or removing the plugin.
[Classic Metamod unload path](https://github.com/alliedmodders/metamod-hl1/blob/18a10db686702e8ae9e0fcb5b5febf8881dc9c2d/metamod/mplugin.cpp#L943-L1020) ·
[Metamod-R unload path](https://github.com/rehlds/Metamod-R/blob/master/metamod/src/mplugin.cpp#L429-L495)

On Win32 the export macro uses stdcall for `GiveFnptrsToDll` and cdecl
for the Metamod entry points. Metamod resolves these exports by name.
The SDK leaves PE ordinals to the linker.

## Native memory and registration contracts

- The SDK allows 256 irreversible console registrations per attachment,
  shared by commands and CVars. Names are reserved before the native call.
  A rejected CVar attempt consumes its reservation, so retrying the same
  name cannot allocate another retained record. The native registration
  API returns no ownership or failure status. The SDK keeps submitted
  storage resident. CVar records occupy write-once slots in a bounded,
  module-resident pool. Metamod-R uses the record address to identify the
  registering module. The SDK never reads a record back after handing it
  to the host. Classic Metamod and Metamod-R copy CVar inputs in
  their registration wrappers. ReHLDS can retain a record passed directly
  to the engine and replace the value pointer with engine storage. It
  unlinks external CVars without freeing the caller's record. Its game
  command path copies the name and stores the callback. The
  SDK relies on the same conservative retention contract for classic
  HLDS, whose implementation is closed source. A successful name lookup
  establishes presence only.
  [Metamod-R registration wrappers](https://github.com/rehlds/Metamod-R/blob/master/metamod/src/reg_support.cpp#L16-L69) ·
  [ReHLDS CVar registration and unlinking](https://github.com/rehlds/ReHLDS/blob/master/rehlds/engine/cvar.cpp) ·
  [ReHLDS game command registration](https://github.com/rehlds/ReHLDS/blob/master/rehlds/engine/cmd.cpp#L756-L799)
- Version reads are capped at 64 bytes, command tokens share a 4,096-byte
  budget, and other native string copies are capped at 4,096 bytes per
  value, including the terminator. Null or over-limit values are treated as
  absent. These limits bound reads. The host still supplies a stable,
  initialized pointer readable through the first NUL or the limit,
  whichever comes first. A Metamod hook or registered command observed
  during a native string getter invalidates the pending read before Rust
  touches the returned pointer. The host must preserve the returned
  buffer and the active command context across activity the SDK cannot
  observe.
  Metamod utility messages are capped at 1,021 bytes to leave room for
  the terminator and console newline in classic Metamod's formatting
  buffer. `Server::print` continues to pass its C string directly to the engine.
- Engine data is read on the server thread. Native edict slots must remain
  readable for their map epoch, and the host must report map transitions
  before discarding the arena. The SDK checks player sessions, edict serials,
  and map epochs around native calls. Player information requires a
  valid client capacity and the edict serial observed at join, even if
  a replacement at the same index has not yet produced a player hook.
  Player slot lookup rejects indices above the known client capacity.
  Early hooks may arrive before a capacity snapshot is available.
  Entity access becomes active after the outermost paired activation hook
  returns from the GameDLL. The host must deliver matching pre and post
  hooks in stack order. Deactivation cancels publication for any
  outstanding activation, and shutdown permanently revokes access.
  Join and disconnect tracking check the original edict's live flag and serial
  again after the native slot lookup. A disconnect removes a tracked session
  only if its serial matches the edict. A map transition, shutdown, or nested
  player lookup invalidates that attempt before the second read. The host
  keeps the slot allocation readable until such a boundary, including
  during map loading if it supplies a player hook then.
  `player_at` and `is_player_current` describe observed hook bookkeeping.
  It copies strings before making
  another native call. Native return buffers must remain readable until
  that copy completes. Alignment checks can reject malformed inputs, but
  cannot establish that an aligned pointer refers to readable memory. In
  particular, if the map remains active throughout lookup or tracing,
  the host must return a current-map edict or null. It must keep edict
  arguments valid whenever native code uses them, including when a
  native wrapper reenters before forwarding a call. The host must keep
  an edict slot allocated while its map is active and
  change the slot's serial when it is recycled. A serial that wraps within
  the same map can make an ancient handle indistinguishable from a newer one.
  String-table offsets copied from entity fields must stay valid for
  lookups throughout that map, including across reentrant field changes.
- Metamod provides aligned initialized ABI data, valid callback functions,
  and writable hook-result globals through the return of each active
  hook. This includes outer hooks when shutdown is observed reentrantly.
  The lag-compensation post hook reads an original result only when the
  host supplies a non-null, aligned pointer to an initialized C `int`
  and the original call was not superceded. It copies the value before
  invoking user code.
  Native callbacks must obey the declared calling convention and must not
  throw or use `longjmp` across Rust frames.
  The SDK revokes engine access on shutdown and defers dropping plugin
  state until its active callback returns. Startup-only loading and
  `PT_NEVER` remain in effect.

## Typed hooks and coverage

The [API coverage matrix](api-coverage.md) tracks generated ABI tables,
safe wrappers, and verification. The engine demo opts into both
`AllowLagCompensation` phases through `Plugin::LAG_COMPENSATION_PRE` and
`LAG_COMPENSATION_POST`. It observes the result without changing gameplay.
Pre hooks can ignore, handle, override, or supercede. Post hooks can ignore,
handle, or override. A nested synchronous decision is ignored, while owned
notifications raised during the callback still use the deferred queue.

## Verify

With the initialized submodules and a working 32-bit C++ toolchain and
runtime:

```sh
verify/linux-i686.sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

On Windows, run the following from the x86 Native Tools Command Prompt with
PowerShell 7.4 or newer and LLVM's `bin` directory on PATH:

```bat
pwsh -NoProfile -File verify/windows-i686.ps1
```

The platform scripts compare C++ and Rust sizes, alignments, and field offsets.
They check both example binaries for the four named exports and load each
plugin with `dlopen` or `LoadLibrary`. The shared harness exercises metadata
and lifecycle negotiation, hook results, player-slot translation and
revalidation, CVar updates, command reentry, and deferred callback ordering.
It also checks line and hull tracing, discovery across vacant slots,
out-of-range lookup, spatial snapshots, and interruption by map changes.
Rust tests cover identity invalidation, dispatch limits, off-thread dispatch,
and panic containment during callbacks and cleanup.
The Linux i686 CI job sets `METAMOD_SANITIZE_LOADER=1` to instrument the C++
loader with AddressSanitizer and UndefinedBehaviorSanitizer. Set the same
variable when running the Linux script locally. This requires the 32-bit
sanitizer runtimes and leaves the Rust libraries uninstrumented. The
32-bit GCC AddressSanitizer runtime also checks for leaks. The loader
marks only the specific CVar and command allocations it receives that
the SDK intentionally retains for process lifetime.

Set `METAMOD_SANITIZE_PLUGIN=1` to build the Rust plugins with nightly ASan
and run them under the Clang-instrumented C++ loader. This mode requires
the nightly i686 Rust target, matching Clang and Clang++ drivers, and the
32-bit Clang sanitizer runtimes. Set `METAMOD_SANITIZER_CLANG` and
`METAMOD_SANITIZER_CXX` if the drivers have versioned names. The loader
provides the ASan runtime for the plugins and keeps leak detection enabled.

The separate Linux Rust sanitizer job runs library tests with nightly
AddressSanitizer on x86_64 and i686. Both variants instrument the target
Rust crates and use the prebuilt standard library. For x86_64, run:

```sh
ASAN_OPTIONS=detect_leaks=1 RUSTFLAGS='-Zsanitizer=address --cfg metamod_lsan' \
  cargo +nightly test --locked --workspace --lib --target x86_64-unknown-linux-gnu
```

Leak detection is enabled. Test-only annotations identify the specific
registration allocations and secondary panic payloads that the SDK
deliberately retains. Unrelated leaks still fail the job.
The i686 nightly target supports address instrumentation but does not ship
its runtime. CI supplies the 32-bit Clang runtime externally. With Clang,
its 32-bit compiler-rt libraries, and a multilib linker installed, run:

```sh
ASAN_OPTIONS=detect_leaks=1 \
  RUSTFLAGS='-Zsanitizer=address -Zexternal-clangrt -Clinker=clang -Clink-arg=-fsanitize=address --cfg metamod_lsan' \
  cargo +nightly test --locked --workspace --lib --target i686-unknown-linux-gnu
```

The Rust tests exercise FFI code with test fixtures. The platform loader
jobs separately check the shipping ABI and example libraries.
The Linux script accepts two optional paths for comparing against separate C++
reference checkouts. Binding generation always uses the submodules.

Validated against alliedmodders/metamod-hl1 at
`18a10db686702e8ae9e0fcb5b5febf8881dc9c2d` and alliedmodders/hlsdk at
`a0edb7792a96998d349325bebab8ea41ec5cb239`.
An earlier Ricochet smoke test on Metamod-P 1.21p109 confirmed startup,
the first frame, and a player greeting. The current examples passed the
Linux i686 ABI and loader harness. Win32 and live-server tracing and
entity discovery remain unverified.

Before publishing the SDK, run the example on isolated servers with both
Metamod loaders, including a native Win32 server, and establish the supported
Linux libc baseline. Add further typed hooks and scoped capabilities as
their ABI and lifetime contracts are verified.

## Licensing and references

The workspace retains its [AGPL-3.0-only license](../LICENSE) for SDK-authored
code. The repository records Git links to the upstream revisions. Header
notices and licenses remain in their respective submodules. Bindgen is
BSD-3-Clause licensed and generates declarations locally. The generated files
stay in the build directory, and Cargo publication remains disabled.

Submodules preserve upstream provenance. They do not resolve redistribution
rights for archives that include the headers, generated Rust declarations,
or binaries that incorporate them. Review the upstream terms and the actual
release contents before publishing any such artifact. In particular, review
Valve's redistribution restrictions and notice requirements, and whether
GPL obligations apply to output derived from Metamod headers. Metamod's
linking exception addresses linking with Valve's engine and mods.

- Official Metamod header material by Will Day, copyright © 2001–2003, is
  GPL-2.0-or-later with an upstream exception for linking with Valve’s
  Half-Life engine and Valve MODs. [Metamod interface header and notice](https://github.com/alliedmodders/metamod-hl1/blob/18a10db686702e8ae9e0fcb5b5febf8881dc9c2d/metamod/meta_api.h)
- The Half-Life SDK is copyright © Valve Corp. Its source has separate
  redistribution terms. [HLSDK license](https://github.com/alliedmodders/hlsdk/blob/a0edb7792a96998d349325bebab8ea41ec5cb239/LICENSE)
- Metamod-R uses GPLv3. Its loader was checked for the required exports
  and interface version. [Metamod-R](https://github.com/rehlds/Metamod-R/tree/56590c5c70fa95775051888b7fe0a4e285856ebb)
- The FWGS template illustrates Metamod plugin hook registration and Win32 exports.
  The reviewed revision had no top-level license granting reuse of the
  template. [FWGS Metamod plugin template](https://github.com/FWGS/metamod-plugin-template/tree/b9dbed28728722e8fc751200f824f72e3b8e7123)

## GoldSrc runtime audit

The `metamod-audit` plugin and Python runner exercise the SDK against
independently configured local GoldSrc servers and produce comparable
JSONL, JSON, and Markdown reports. See the [runtime audit guide](runtime-audit.md)
for the matrix, isolated staging, and manual console protocol.

The [Ricochet core v2 certificate](certifications/ricochet-ubuntu-core-v2/20261122T115759Z-6890/certificate.md)
records six certified Linux i686 configurations tested on Ubuntu 26.04. It
lists launcher and engine module hashes separately and applies to the
recorded binaries and map itinerary. The runtime audit guide also documents
the optional Ubuntu wrapper for reproducing the run with a prebuilt plugin.

See the [memory-safety audit guide](memory-safety-audit.md) for the
unsafe boundary review, Miri mocks, sanitizer fixtures, and opt-in live
Memcheck evidence.
