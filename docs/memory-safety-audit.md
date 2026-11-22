# Memory-safety audit

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
| Strings lent to native calls | Console and log calls lend valid C strings for the synchronous call. Metamod variadic utilities receive a fixed format. The new mock triggers nested shutdown while reading a buffer owned by the active plugin and checks that it can still read that buffer before returning. | The native callback must respect const input, its length or termination, and the synchronous borrowing period. Native format or buffer defects remain a host risk. |

Miri found invalidated pointer tags in two Rust mocks. The mock host wrote
to the stack variable holding the hook result between invocations, then the
SDK wrote through its retained raw pointer. The fixtures now use the same
raw pointer for host writes between callbacks. The production path already
receives raw pointers from Metamod. No new production undefined behavior
was confirmed during this pass. The host obligations above are still
prerequisites for soundness. Null and alignment checks do not establish
pointer validity.

## Controlled Rust and native fixtures

Initialize the pinned upstream submodules and install libclang as described
in the README. On a development machine with nightly Miri:

```bash
rustup component add miri --toolchain nightly
python3 verify/audit/memory_safety.py "/tmp/miri-audit-$(date +%s)"
```

The collector requires each selected test to report exactly one passing
test. It preserves the Miri version, commands, input source hashes, logs
and their hashes. CI runs it on Linux x86-64. Admission tests cover bounded
rejection output, nested calls, panic containment, and shutdown during
admission. The other selected mocks exercise
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

The development run passed the eight Miri scenarios, workspace tests and
doctests, Clippy, the i686 ABI and both sanitizer loader variants, and the
Rust ASan/LSan tests on i686 and x86-64. The Python audit suite passed
43 tests with one Windows-only skip. Valgrind 3.25.1 produced a clean XML
report for a static i686 smoke fixture. The dynamic i686 smoke fixture
stopped before launch because the local 32-bit loader lacks the required
debug symbols. No GoldSrc server was instrumented. Win32 verification
requires a Windows SDK and host runner.
