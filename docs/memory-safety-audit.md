# Memory-safety audit

For the contracts, see the [boundary review](#unsafe-boundary-review). To reproduce
checks, use the [fixtures](#controlled-rust-and-native-fixtures) or
[live Linux instrumentation](#instrument-an-existing-linux-matrix).

The **memory-safety v1** evidence format keeps three independent assessments.
It has no aggregate pass or memory-safety badge.

| Assessment | Evidence and scope |
| --- | --- |
| Compatibility | Continue to use [the existing certification evaluator](runtime-audit.md#compatibility-certification). Core v2 verifies specified behavior on identified binaries and maps. |
| Rust SDK soundness | Review the unsafe boundary against the host obligations below. Use Miri on Rust mocks and the instrumented native fixture to test specific invariants. Passing those checks leaves the full soundness assessment **INCONCLUSIVE**. |
| Native runtime | Use Memcheck on the selected live Linux i686 rows. A **PASS** covers the instrumented paths on that exact deployment. Findings and their attribution belong to this dimension. |

A clean compatibility, Miri, or sanitizer result cannot replace any of the
other evidence. Historical compatibility certificates retain their original
scope.

## Unsafe boundary review

| Boundary | SDK invariant and evidence | Host obligation and remaining risk |
| --- | --- | --- |
| Bindgen, loader exports and hook table factories | Bindgen uses the pinned headers. Typed export assignments and the x86 native layout and symbol checks verify the expected calling convention and layouts. The loader copies the used callbacks instead of borrowing the table through reentry. The first engine handoff fixes thread ownership. | The loader supplies aligned, initialized storage and valid function pointers for the expected ABI. Tables used as outputs have space for the entire structure. Engine globals and callbacks remain live through shutdown. |
| Hook results and original returns | The SDK copies the lag-compensation original return before entering plugin code and writes the current result via a raw pointer. Ignored and panic paths are covered in the fixture. | Metamod keeps the result storage writable through nested callbacks and supplies an initialized original integer for that post hook. Off-thread hooks get no Rust dispatch or result write. |
| Plugin state, panic and shutdown | The dispatch guard permits one mutable plugin borrow at a time. Nested notifications carry owned values. Shutdown revokes engine access immediately and takes the instance only after the active callback ends. The shutdown callback and destructor have independent panic containment. | Runtime and forced unloading remain unsupported. The native host must stop invocation before removing code or tables. Destructors are best-effort if the host never forwards shutdown. Abort-on-panic builds and process termination also prevent cleanup. |
| Edict arena, traces and player identity | Indices are bounded by current capacities. Epoch, native serial and tracked player session are checked at lifetime boundaries. Controlled mocks retire an arena during lookup and discard the expired pointer. Trace inputs and output storage live through the synchronous call. | The host must deliver map retirement before invalidating the arena and keep same-map slot identity fields readable. Native functions must not retain trace stack pointers or use a skip edict after invalidating it. Unobserved retirement and incorrect native serial bookkeeping cannot be detected reliably. |
| Native strings and validated byte types | SDK scans stop at the NUL or configured limit. Engine strings are copied before another native call. Observed SDK reentry invalidates borrowed getter results. Transparent validated wrappers preserve the original allocation, DST metadata and borrow. | A non-null returned string needs contiguous initialized bytes through its NUL or the read limit. Native code must preserve that range while Rust reads it. Unobserved reentry in other plugins or internal native code may violate the getter lifetime. |
| CVar and command registration | The CVar pool reserves disjoint writable slots atomically, initializes each once and leaves it resident. Names, defaults and command trampolines outlive all supported uses. No registry borrow crosses native code. The native fixture verifies the module ownership lookup used by Metamod-R. | Native registration may retain records but the host must follow its ownership contract for the original allocations. The SDK cannot recover records or deregister commands portably. Registration storage is bounded and intentionally retained. |
| Strings lent to native calls | Console and log calls lend valid C strings for the synchronous call. Metamod variadic utilities receive a fixed format. A controlled mock triggers nested shutdown while reading a buffer owned by the active plugin and checks that it can still read that buffer before returning. | The native callback must respect const input, its length or termination, and the synchronous borrowing period. Native format or buffer defects remain a host risk. |

Miri found invalidated pointer tags in two Rust mocks. The mock host wrote
to the stack variable holding the hook result between invocations, then the
SDK wrote through its retained raw pointer. The fixtures now use the same
raw pointer for host writes between callbacks. The production path already
receives raw pointers from Metamod. No new production undefined behavior
was confirmed during this pass. The host obligations above are still
prerequisites for soundness. Null and alignment checks do not establish
pointer validity.

The server-state snapshot reads only selected initialized globals on the
owning thread after activation. It copies scalar values before any native
call and resolves map/start strings through the existing bounded getter
path. Observable reentry or map/shutdown invalidation discards the pending
snapshot before reading a returned string pointer. The host must initialize
these fields by post-activation, retain globals through shutdown, preserve
same-map string indices, and keep returned strings readable through the
first NUL or read limit while copying. Rust cannot detect hidden native
invalidation. Private globals pointers, trace scratch state, and borrowed
engine references are outside the API.

## Entity relationship contracts

The pinned [entvars fields](../upstream/hlsdk/engine/progdefs.h) store
`owner`, `enemy`, `aiment`, and `groundentity` as plain edict pointers.
[ReHLDS edict release and reuse](https://github.com/rehlds/ReHLDS/blob/master/rehlds/engine/pr_edict.cpp)
increments the released slot's serial and clears its entvars when allocated
again; it does not repair pointers held in other entities. The ABI provides
no original-target serial in these fields. Valve HLDS's private implementation
is not established by the ReHLDS source review.

Relationship snapshots copy the source pointer values, then compare them
with pointers returned by bounded engine lookups. Only lookup results are
dereferenced, under the existing current-map edict contract. Foreign or
retired relationship addresses need not be readable. The host must initialize
the source pointer fields and retain same-map slot storage, and must notify
map retirement before freeing that arena. No check can establish arbitrary
foreign allocation validity or detect a hidden host violation.

The source is revalidated after every native call and its relationship
fields are checked again before return. Known map/shutdown interruption
discards results before accessing retired storage. Targets are observations
of current occupants, not proof of historical relationship intent; saved
handles carry the observed epoch and serial. The scan is not atomic.

Rust/Miri fixtures cover unrelated addresses, null and vacant targets,
self-links, source replacement, target reuse, changed relationships, map
retirement and shutdown. The shared x86 loader checks world/self links,
vacant/unrelated targets and map interruption through the demo command.
These synthetic checks do not establish live host behavior. Separate
[Linux live tests](entity-relationships.md) recorded ordinary removal/reuse
and map changes on three identified host combinations, without instrumentation. Neither
set of results extends the historical compatibility certificate.

## Controlled Rust and native fixtures

Model and sound [precaching](resource-precaching.md) requires static validated
names because classic implementations can retain the original pointer.
The SDK allocates no registration storage. The worldspawn scope is synchronous,
not deferred; lifecycle interruption prevents publishing a returned index and
blocks subsequent calls through that scope. The host must implement the reviewed
worldspawn/loading contract and must not unwind or jump across Rust frames on
native failure. Asset validity and native resource capacity remain host concerns;
these calls cannot promise recovery from fatal engine errors.

Initialize the pinned upstream submodules and install libclang as described
in the [SDK reference](sdk-reference.md#linux-i686). On a development machine with nightly Miri:

```bash
rustup component add miri --toolchain nightly
python3 verify/audit/memory_safety.py "/tmp/miri-audit-$(date +%s)"
```

The collector requires each selected test to report exactly one passing
test. It preserves the Miri version, commands, input source hashes, logs
and their hashes. CI runs it on Linux x86-64. Admission tests cover bounded
rejection output, nested calls, panic containment, and shutdown during
admission. Borrowed-reason regressions exercise plugin-state mutation and
destruction during deferred callbacks and borrowing from the connection
snapshot. The reason bytes are copied before releasing the plugin borrow.
Scoped-send mocks check exact bytes, validation, nested-send rejection,
lifecycle interruption, and guard release after a Rust unwind.
The other selected mocks exercise
nested shutdown during borrowed console output, callback panic and
reentrancy, player slot reuse, expired CVar strings, and a retired edict
arena. They execute Rust callbacks with the C calling convention. Miri
does not load the game engine or the C++ harness. Run the existing native
fixture and sanitizer checks separately:

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
python3 -m unittest discover -s verify/audit -v

METAMOD_SANITIZE_LOADER=1 bash verify/audit/collect-native.sh /tmp/native-loader linux-i686-loader
METAMOD_SANITIZE_PLUGIN=1 bash verify/audit/collect-native.sh /tmp/native-combined linux-i686-combined
```

The first native command instruments the C++ loader with ASan, UBSan and
LSan. The second additionally builds the Rust plugins with ASan and uses
one shared Clang sanitizer runtime. The existing CI matrix runs Rust
ASan and LSan tests independently on Linux i686 and x86-64. These fixtures
are synthetic. The Win32 ABI and loader job remains a separate check.

## Memory Contracts Validation v1: initial probes

`native.cvar.retained_storage.v1` is a synthetic native loader probe. It saves
pointers to the registered CVar record, name and initial default buffer before
the fixture replaces the record's string pointer. After plugin shutdown it
checks the record and both original strings, writes and restores one byte in
the original default buffer, and only then emits its PASS record. The module
stays loaded throughout; forced unloading remains unsupported.

The strict native importer requires this probe alongside the existing suite.
Missing, duplicate, conflicting, malformed or unexpected records invalidate
strict imports, as do sanitizer diagnostics and unsuccessful process exits.
The `collect-native.sh` commands above exercise the probe in the existing
Linux loader and combined sanitizer jobs; the shared Windows loader also
executes it, with Windows execution still pending.

This checks SDK retention under a controlled host that replaces the string
pointer without freeing the initial allocation. Native hosts must honor the
registration ownership contract and keep callback code loaded. It does not
establish arbitrary pointer readability or ownership on a live engine.
`native.string.reentrant_invalidation.v1` extends the existing Rust/Miri
string invalidation scenario into the native loader. The CVar getter selects a
heap string, synchronously invokes the SDK frame hook, frees the selected
string, and returns its expired pointer. The command must report engine
unavailability; a subsequent ordinary CVar read must succeed before PASS is
emitted. Strict imports require this record exactly once, with regression
cases for missing and duplicate evidence.

The observed SDK callback invalidates the getter read even though the map and
plugin remain active. The fixture intentionally returns an expired pointer to
exercise rejection before dereference. C++ loader ASan can detect a read from
this C++ allocation only when that read is instrumented; use the combined
Rust-plugin ASan run to instrument the SDK's reads. The existing Miri mock
provides separate Rust evidence. This does not cover unobserved native reentry:
hosts still must preserve returned strings while Rust reads them.

`native.entity.retired_arena.v1` adds native allocation retirement to the
existing map-transition discovery checks. At slot 3 the lookup allocates an
edict, notifies the SDK of map deactivation, frees that edict, and returns its
expired pointer. Discovery must report the map change without publishing any
snapshot. The fixture checks that this interruption occurred and reactivates
the map before emitting PASS. Strict imports require the record exactly once;
missing and duplicate records have dedicated rejection cases.

This represents one retired arena element, rather than replacing the entire
engine arena. Combined Rust/C++ ASan instruments the SDK reads that would
otherwise touch the freed edict. Hosts must notify retirement before freeing
arena storage, keep same-map identity fields readable, and maintain native
serials correctly. Unobserved retirement remains outside this check.

Strict Linux native imports now require a version 1 build manifest.
The shared strict probe-suite check also runs on Windows, whose collector does
not yet produce this manifest. Windows imports explicitly record
`runtime.native_build_identity.status = "UNVERIFIED"`; they do not claim verified
build provenance. Windows run/build identity binding remains pending.

The Linux collector assigns a run UUID, retains the loader and three plugin binaries, and records
their SHA-256 hashes, target, compiler versions, sanitizer selection, environment
options, repository/submodule commits, dirty status, and tracked diff hashes.
Collection rejects source-state changes during compilation and requires the
pinned repository header paths. Effective sanitizer options are recorded.
The build manifest is emitted into the transcript before fixture execution.
`runtime.json` binds the manifest and complete loader log by SHA-256. Import
checks those hashes, the retained artifacts, target, run UUID, and exactly one
matching transcript manifest before accepting strict evidence. The verified
manifest is preserved in the report as `runtime.verified_native_build`.

Use a fresh output directory for every collection; existing evidence is refused.
Archive the whole directory, including `artifacts/`, `build.json`, `runtime.json`,
and `loader.log`. Reimport with the same command used by `collect-native.sh`.
Old transcripts without this binding remain usable for non-strict historical
inspection but cannot satisfy a new strict import. Dirty status is explicit;
a commit identifies the base revision, not a clean-build claim. Diff hashes
cover tracked changes; untracked source contents are not archived. This is
local integrity checking, not cryptographic attestation of compiler behavior
or protection against deliberate rewriting of the whole bundle.

The remaining versioned memory-contract probes are still pending; this increment
does not complete the v1 profile or alter
compatibility certification.

## Instrument an existing Linux matrix

On the independent test host, install Valgrind with i386 Memcheck support
and the matching 32-bit loader debug symbols if required by the package.
Use the existing saved matrix and verified prebuilt plugin. Start with one
ReHLDS row before running the full matrix. Allocate substantially more
time and disk space than for the uninstrumented run.

```bash
MATRIX="$HOME/ricochet-audit-lab/runs/ACTUAL_RUN_ID/matrix.json"
OUT="$(mktemp -d "$HOME/ricochet-memcheck.XXXXXX")"
python3 verify/audit/runner.py run "$MATRIX" "$OUT/run" --platform linux \
  --only linux-rehlds-metamod-r-stock --memcheck
```

Use a fresh output directory and omit `--only` for all six rows. The
runner uses the existing isolated staging, console driver and artifact
identity checks. Memcheck mode multiplies the configured phase timeout
by four and caps the normal quit wait at 60 seconds. It follows the
initial server process. It ignores user Valgrind configuration, uses the
installed tool's default suppressions, and records their hit counts.
It reports definite, indirect, and possible leaks and leaves reachable
allocations outside the finding set. [Valgrind core manual](https://snapshots.sourceware.org/valgrind/trunk/latest/html/manual-core.html)

The Ubuntu bootstrap can also rerun a saved matrix. From a clean checkout
that includes Memcheck support, run the following on the audit host.

```bash
bash verify/audit/ricochet_audit_ubuntu.sh \
  --rerun 20261122T115759Z-6890 --rerun-runner-checkout "$PWD" --memcheck
```

Use `--workdir PATH` if the saved run lives outside the default workspace.
The explicit checkout is needed for a saved run whose pinned runner
predates Memcheck. When the saved runner already supports it, omit
`--rerun-runner-checkout` to keep the original pin. Reruns leave the source
run untouched, reuse its binaries, and record both runner revisions.
Install Valgrind and any matching i386 loader debug symbols separately
before a rerun. The script performs no package installation in rerun mode.

Omit `--certify` to collect memory-safety evidence alone. If both flags are
supplied, the new run writes memory-safety reports under `report/` and
the independently evaluated compatibility certificate under
`certification/`. The original core v2 certificate stays in the source
run. `--failed-only` retains its audit or certification selection policy.
Use `--only` to target a particular Memcheck row.

Read `memory-safety.json` and `memory-safety.md` beside the ordinary audit
report. Preserve `memcheck.xml`, `launch.json` and the console logs for
each row. Inspect the Memcheck stacks and allocation origins with the
exact SDK and upstream binaries or matching debug symbols. A frame in the
SDK alone cannot establish ownership of a fault. v1 leaves every detected
finding **UNRESOLVED** pending manual attribution. A confirmed upstream
defect remains a native finding and does not alter the Rust assessment.

Native status rules:

- **PASS** requires matching PID and executable in supported Memcheck XML,
  a finished process with zero reported errors, verified artifact identities,
  completed configured map visits, clean quit and intact capture.
- **FAIL** records Memcheck errors from matching diagnostics, even when the
  audit itinerary is incomplete.
- **INCONCLUSIVE** covers missing final diagnostics after launch, malformed
  or oversized XML, unsupported XML versions, incomplete capture or
  uncorroborated identities. The original diagnostics stay on disk.
- **NOT_RUN** covers failures before instrumentation could start.
- **UNSUPPORTED** covers a non-Linux row or host, missing Valgrind, or a
  row already marked unsupported. Installed Valgrind may still lack i386
  support or fail to load the server, which requires inspecting stderr.

An opt-in run exits nonzero if any row lacks a native **PASS** or the
ordinary audit fails. For compatibility, use the unchanged `runner.py certify`
command against the ordinary report and preserve its certificate separately.

## Validation of the v1 tooling

The following records the initial tooling validation, rather than a current
workspace test count.

The development run passed the eight Miri scenarios, workspace tests and
doctests, Clippy, the i686 ABI and both sanitizer loader variants, and the
Rust ASan/LSan tests on i686 and x86-64. The Python audit suite passed
43 tests with one Windows-only skip. Valgrind 3.25.1 produced a clean XML
report for a static i686 smoke fixture. The dynamic i686 smoke fixture
stopped before launch because the local 32-bit loader lacks the required
debug symbols. No GoldSrc server was instrumented. Win32 verification
requires a Windows SDK and host runner.
