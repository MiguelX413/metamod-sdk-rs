# GoldSrc runtime auditing

Start with [setup and commands](#configure-and-run). See [certification](#compatibility-certification)
for recorded results and [interpretation](#interpretation) for their scope.

`metamod-audit` is a resident Metamod plugin built on the public SDK. It uses normal GameDLL hooks, ordinary server commands, and owned SDK snapshots. The Python runner copies an operator-supplied installation, selects one Metamod and one GameDLL, and collects the plugin's observations. The C++ fixture exercises adversarial SDK paths with controlled native inputs.

## Matrix and prerequisites

| Engine | Manager | Priority |
| --- | --- | --- |
| Valve HLDS | alliedmodders/metamod-hl1 | Primary |
| ReHLDS | alliedmodders/metamod-hl1 | Primary |
| ReHLDS | Metamod-R | Primary |
| Valve HLDS | Metamod-R | Experimental |

Each pair has Linux i686 and Windows x86 rows for stock Ricochet and `ricochet-updated`. Half-Life Deathmatch is an optional control. A template row expresses a test target. Compatibility and game-specific exports have to be established for the exact binaries in each run.

Metamod-R's README and current documentation list original HLDS support and recommend ReHLDS API 3.1 or later. Its upstream game-test repository describes dropping old WON-era games from its ReHLDS/Metamod-R testing. The latter describes a narrower test scope and potentially confusing Steam-only language. Treat Steam-era Valve HLDS, older WON builds, and ReHLDS API versions as distinct configurations. ReHLDS also warns that native modules relying on original HLDS internals can be binary-incompatible despite GameDLL/API compatibility claims. Record the precise build and keep crashes or missing exports as evidence for that row. [Metamod-R README](https://github.com/rehlds/Metamod-R#readme) · [Metamod-R documentation](https://rehlds.dev/docs/metamod-r/) · [Upstream game-test scope](https://github.com/rehlds/metamod-r-game-tests#readme) · [ReHLDS README](https://github.com/rehlds/ReHLDS#readme)

You need:

- A dedicated local test installation provisioned through Valve's Steam tooling, with legal copies of the appropriate mod assets, maps and stock GameDLL. SteamCMD users can provision Half-Life Dedicated Server (app 90) in a dedicated directory and select the appropriate mod. Check the installed files and the server's `version` output before configuring the matrix. The runner has no download step. [Valve SteamCMD documentation](https://developer.valvesoftware.com/wiki/SteamCMD)
- Independently provisioned x86 manager binaries and an x86 GameDLL for every selected row. Build `ricochet-updated` from the chosen revision and record that revision and build details. The upstream project targets VS2019, is archived, and provides no guarantee for a particular Linux build. Leave an unavailable build explicitly untested. [Classic Metamod source](https://github.com/alliedmodders/metamod-hl1) · [ricochet-updated source](https://github.com/twhl-community/ricochet-updated)
- Python 3.10+ and a host able to execute its OS's x86 binaries. Linux needs the installation's 32-bit runtime dependencies. Windows needs its GameDLL and Metamod dependencies. For `--build`, install the Rust i686 target and the repo's normal C/C++/libclang prerequisites, including the x86 Windows SDK on Windows. The Windows build uses the same MSVC target as the existing ABI checks.
- Trusted, stand-alone test binaries and configs, sufficient space for a full copy, and an unused loopback server port. The copied server executes its own startup configs, so use an installation with clean test configs and no external redirects, credentials, custom startup scripts or unrelated plugins. Staging provides filesystem isolation from the source installation through copying. It is a test environment, not an OS security sandbox. Use a separate OS account or VM if the runtime needs stronger isolation.

## Configure and run

The Ubuntu bootstrap materializes valid internal file and directory links
in both server snapshots. It rejects escaping paths, cycles, broken links,
and special files before copying. Only the root-level `libSDL2.so` alias
can use the Ubuntu i386 SDL2 library. The exception applies to a missing
`libSDL2-2.0.so.0` beside the alias or an exact link to that library in
`/usr/lib/i386-linux-gnu`. The bootstrap checks the replacement's i686 ELF
header and SDL2 SONAME. The Steam tree remains intact, including on
`--skip-steam` provisioning. Restore any other rejected dependency in the
source tree before retrying. Staging checks file layout and the fallback
ABI, while library loading is checked during the actual server run.

From the repository root:

```sh
python3 verify/audit/runner.py template --hldm > /path/to/local/matrix.json
python3 verify/audit/runner.py run /path/to/local/matrix.json /path/to/new/report-dir \
  --only linux-hlds-classic-stock --build
```

Edit the generated rows before use. Keep only rows you intend to run, or select them with repeated `--only` arguments. `--platform linux` and `--platform windows` filter a shared matrix on native hosts. The same schema accepts new games by adding a row with a new `id`, its mod directory, compatible binaries, and maps. The template uses illustrative paths. Use actual platform-specific filenames, including the correct `liblist.gam` entry and engine module for your installation.

| Field | Meaning |
| --- | --- |
| `schema_version` | Configuration version `1` |
| `runtimes[].id` | Unique stable comparison key |
| `os`, `installation`, `mod` | Target OS, source tree relative to the configuration or absolute, and mod directory beneath that tree |
| `engine`, `metamod`, `gamedll` | Each entry has an independent `implementation`, exact `version` or source revision, and installation-relative `binary`. Optional `sha256` pins that artifact. |
| `engine.modules` | Additional relative paths to hash, such as `swds.dll` or `engine_i486.so`, when the launcher is separate from the engine |
| `engine.library_dirs` | Optional installation-relative directories to add to the staged library search path |
| `maps`, `port`, `timeout_seconds` | One to eight map names, a dedicated loopback port (default 27045), and per-readiness or command timeout (default 45 seconds) |
| `plugin` | Optional prebuilt plugin path relative to the configuration or absolute. Report its actual `audit_plugin_commit` and `sdk_commit` in the same row. An optional `plugin_sha256` pins the binary. |
| `plugin_provenance` | Build provenance manifest for a prebuilt plugin. Required for certification. See below. |
| `experimental` | Records experimental scope in the report without changing how failures are evaluated |
| `unsupported_reason` | Optional evidence-backed exclusion. The runner records the row as unsupported and skips launch. |

Without `plugin`, `--build` compiles for the selected target into the output tree and records the repository HEAD and dirty state. Otherwise the runner looks in the standard Cargo release target directory. For a prebuilt binary with unknown provenance, commit fields remain null. Hashes identify actual files. Version labels are operator attestations, and the runner records matching lines from `version` and `meta version` separately for review. It does not derive any one component's identity from another.

The runner validates x86 binary headers and maps, rejects source-tree symlinks and reparse points, then modifies only its copy. It routes the staged mod's platform `liblist.gam` entry through the selected manager and supplies isolated `mm_configfile`, `mm_pluginsfile`, and `mm_gamedll` settings. It starts a LAN server on loopback, waits for the first-frame readiness event, then verifies console round trips. Later maps are requested with `changelevel` and checked for a new activation event before replaying the suites. Normal exit uses `quit`; timed-out processes are terminated as necessary. Standard output and error are captured separately, each capped at 64 MiB. Available staged crash dumps are copied with count and size limits. Dumps handled by a system crash collector outside the staging directory need to be collected using that system's tools.

Only use `--client-wait SECONDS` when an operator is available to connect a local game client. Neither player manipulation nor server shutdown uses a forced plugin unload. The server copy is removed after normal automated runs. An unavailable installation, missing readiness, failed console transport, process crash, protocol loss, sanitizer diagnostic, or FAIL yields a nonzero exit.

## Compatibility certification

The [November 22, 2026 Ricochet run](certifications/ricochet-ubuntu-core-v2/20261122T115759Z-6890/certificate.md)
certified all six tested Linux i686 configurations under core v2. The
[JSON certificate](certifications/ricochet-ubuntu-core-v2/20261122T115759Z-6890/certificate.json)
preserves the report digest, component identities, and probe results. Its
`<original-run-root>` prefix replaces the operator's local run path. The
source report and console logs remain with the original run. Both formats
record the shared `hlds_linux` launcher hash and the distinct Valve HLDS
and ReHLDS `engine_i486.so` hashes.

For a fresh Ubuntu 26.04 x86_64 audit with a prebuilt Linux i686 plugin
and its existing build manifest, `certify-ricochet-ubuntu.sh` fetches a
runner revision into `~/ricochet-audit-bootstrap` and delegates fresh
provisioning and core certification to `ricochet_audit_ubuntu.sh`.
Run it as a non-root user with sudo access in a dedicated test container.
The command below selects the historical runner revision for the archived
run. Select a newer revision to use its current report presentation.

```bash
bash verify/audit/certify-ricochet-ubuntu.sh \
  --plugin "$HOME/libmetamod_audit.so" \
  --manifest "$HOME/audit-build.json" \
  --repo-ref b9645b5bcf3940d28573881b2d12fbfa9be2142c \
  --updated-gamedll "$HOME/ricochet_updated.so" --require-six
```

The manifest must describe the actual plugin build. The runner ref can
differ from that build revision. The wrapper also detects
`$HOME/ricochet_updated.so` when `--updated-gamedll` is omitted. If neither
is available, omit `--require-six` to run the three stock configurations.

Audit execution and certification are separate. Run the automated matrix
from a clean runner checkout, writing the report outside the checkout. Keep
`report.json` together with the per-runtime directories containing
`events.jsonl` and the original transcripts. Then evaluate a profile:

```sh
python3 verify/audit/runner.py certify /path/to/report-dir/report.json core /path/to/certification
```

The command writes `certificate.json` and `certificate.md` in a new
output directory. It evaluates each runtime independently. Exit status 0
means every row is certified, 1 means one or more rows did not qualify,
and 2 means invalid input or an output error. Both outputs record the
engine executable hash and any configured engine module hashes separately.
For example, two configurations can share `hlds_linux` and load different
`engine_i486.so` binaries. If a row records no separate modules, the
Markdown labels the executable as the engine binary. The certificate's schema,
the audit protocol, and the profile policy each have a separate version.
When requirements change, bump the affected profile's version. An earlier
certificate still describes the earlier policy. Reevaluate the evidence
under the new policy to claim the new version.

| Profile | Mandatory observations in v2 |
| --- | --- |
| `core` | Initialization, command registration and quoted arguments, CVar round trip, owned string copies and size limits, invalid input handling, entity discovery and snapshots, line and hull traces, map activation and deactivation order, rejection of stale handles, and ordered shutdown and destructor entry. |
| `player` | Everything in core, plus join, disconnect, an owned session snapshot, and same-slot reuse by clients classified by the engine as non-fake. Use `--client-wait` and connect, disconnect, and reconnect into the same slot on the same map. |
| `extended` | Everything in player, plus classname keyvalue delivery, pre and post lag-compensation callbacks observed during gameplay, and repeated owned CVar reads. Choose a host and GameDLL that deliver those callbacks, and exercise gameplay with a local client. |

All profiles need corroborated identities for the engine, Metamod, GameDLL,
and plugin. The evaluator requires a complete, untruncated console capture,
a successful `quit`, and a full itinerary with at least two distinct maps.
Every requested transition needs ordered activation and deactivation plus
a stale-handle rejection. Mandatory results must be plugin PASS records
that match the report's sequence references and hash-checked event log.
Manual assertions, imported reports, and synthetic native fixtures cannot
supply those records. Keep native fixtures for separate FFI coverage.

Profile v2 pins the UTF-8 expectation text for each mandatory probe using
SHA-256 fingerprints of the Rust probe catalog. Event and summary agreement
alone cannot validate a changed contract. Issue a new probe ID version when
its expectation or what a PASS proves changes, then update the affected
profiles and bump their versions. Keep existing fingerprints stable.

Entity discovery, entity snapshots, line traces, and hull traces must pass
inside the recorded all-suite command window on **every** map visit.
Initial activation must precede the first suite. Later visits also require
deactivation, activation, ordering, and stale-handle evidence for that
transition. The other mandatory probes are runtime-wide, including
player and gameplay observations. Older reports without suite windows cannot
establish v2 map coverage and need a fresh run.

With `ricochet_audit_ubuntu.sh`, `--rerun latest --failed-only --certify PROFILE`
reevaluates the saved evidence using the policy in the pinned runner
checkout and selects failed, incomplete, unverified, and unreported rows.
Without `--certify`, it retains
the ordinary audit-health filter. Automatic latest selection skips empty
matrices and partially recorded attempts. A saved run with an explicit
selection records the attempted rows separately. Older filtered runs without
that record can still be selected by their explicit run ID.

Ubuntu provisioning refuses tracked edits, staged changes and nonignored
untracked files before switching runner revisions. It checks the fetched
commit and validates all initialized submodules against their Git links.
Reruns require a clean, complete checkout and keep the saved runner pin
unless an explicit `--rerun-runner-checkout` selects another checkout.
They compare saved plugin and component hashes against the binaries on
disk. New matrices pin launcher, engine module, Metamod and GameDLL hashes.
Older matrices can use the previous capture's hashes where available.
The runner revision and the independently declared prebuilt-plugin and
SDK revisions are recorded separately.

The bootstrap also supports opt-in `--memcheck` on saved reruns. Its
memory-safety reports are separate from compatibility certification.
For saved matrices pinned before Memcheck support, use an explicit clean
runner checkout. [Memory-safety audit guide](memory-safety-audit.md#instrument-an-existing-linux-matrix)

The profile is the applicability rule. Player and gameplay probes are
outside core's mandatory scope. Hooks measured only by the native fixture
are outside the live compatibility profiles. If a profile requires a hook and the
host never forwards it, that profile remains incomplete even if the host
intentionally omits the hook. A skipped or unobserved hook does not
establish whether the host supports it. Use a narrower profile to make a
narrower claim.

Each verdict has one of four outcomes, in this precedence order for live
reports:

- **FAILED** means an observed plugin failure, failed transition,
  crash or sanitizer diagnostic, nonzero process exit, or a contradictory
  runtime identity or artifact provenance.
- **UNVERIFIED** means the evidence scope, binary identity, build
  provenance, or event log cannot be verified. Imports and synthetic
  fixtures are ineligible.
- **INCOMPLETE** means live evidence is attributable but coverage or
  lifecycle requirements are missing. A required SKIP or INCONCLUSIVE
  result leaves a coverage gap.
- **CERTIFIED** means the specific row satisfies all requirements of the
  chosen profile version.

For a prebuilt audit plugin, place a build manifest in the matrix row under
`plugin_provenance`, or save it as a JSON file for the Ubuntu wrapper.
Have the artifact build record these fields:

```json
{
  "schema_version": 1,
  "sha256": "<64 lowercase hex characters for the .so or .dll>",
  "audit_plugin_commit": "<full 40-character Git commit>",
  "sdk_commit": "<full 40-character Git commit>",
  "working_tree_dirty": false
}
```

The plugin and SDK revisions may differ from each other and from the runner
checkout. The manifest is a build attestation. The evaluator checks its
digest against the deployed artifact and rejects contradictory revision
fields. It cannot independently derive source revisions from a binary or
authenticate the manifest author. A runner-built plugin uses the clean
checkout revisions recorded at build time. Reports collected before event
log hashes and build provenance were captured may need to be recollected.

```sh
verify/audit/ricochet_audit_ubuntu.sh --plugin ./libmetamod_audit.so \
  --plugin-provenance ./audit-build.json --certify core
```

On Windows, use the same Python `certify` command against the Windows live
report. A certificate records the identities, hashes, profile, and evidence
for the exact binaries and tested itinerary. It establishes neither
compatibility across other configurations nor memory safety or compliance
with all native host contracts.

### Server-state validation

The optional `server-state v1` profile adds `server.state.v1` to the
core v2 requirements. It requires a canonical plugin PASS inside the
all-suite window on every map visit. Missing, skipped, altered, or synthetic
evidence cannot satisfy it. Core/player/extended v2 requirements and
historical certificates keep their original scope.

The probe checks that active-map snapshots contain validated time/frame
duration, capacities, finite mode/flag values, and an owned nonempty map
name. It reports the snapshot as bounded diagnostic text. It does not
prove clock monotonicity, wall-clock correspondence, GameDLL mode semantics,
or native pointer validity. A starting spot may legitimately be absent.
Malformed values and reentry/lifecycle rejection are tested separately
with controlled Rust and native fixtures.

Build the updated audit plugin for the target and record its actual build
revision, dirty state, and SHA-256. Run a fresh isolated matrix with that
binary and at least two distinct maps. Keep the original reports and
plugin provenance intact. The `all` suite includes the new probe;
`mm_audit_run state` exercises it separately for diagnosis.

```bash
python3 verify/audit/runner.py run /path/to/updated-matrix.json /path/to/new-report --platform linux
python3 verify/audit/runner.py certify /path/to/new-report/report.json server-state /path/to/new-certificate
```

The same profile is available on Windows and through the Ubuntu wrapper's
`--certify server-state` option. Rebuilds require a new matrix/artifact
manifest; a pinned rerun cannot substitute a different plugin silently.
An old plugin can still be evaluated under core v2, but its missing state
probe leaves `server-state v1` incomplete. The development observations
below did not qualify for certification.

#### Linux server-state observations

The new probe ran on November 24, 2026, with stock Ricochet on fresh copies
of the saved installations. The itinerary was `rc_arena`,
`rc_deathmatch`, `rc_arena`, `rc_deathmatch`. No client was needed.

| Linux i686 host | State probe | Component identity checks | Shutdown |
| --- | --- | --- | --- |
| Valve HLDS 8684 + classic Metamod 1.21.1-am | PASS on all four visits | PASS | quit, exit 0 |
| ReHLDS 3.15.0.896-dev + classic Metamod 1.21.1-am | PASS on all four visits | PASS | quit, exit 0 |
| ReHLDS 3.15.0.896-dev + Metamod-R 1.3.0.149 | PASS on all four visits | PASS | quit, exit 0 |

The observations included time resetting to 1.0 on replacement maps,
frame duration reaching 0.8 seconds, client capacity 6, entity capacity
975, and the corresponding map names. The probe's finite mode/flag checks
passed; their values remain GameDLL-defined. Empty starting-spot strings
were preserved. Diagnostic text is bounded, so a report line may truncate
the formatted snapshot.

The audit command exited zero. Evaluation under `server-state v1` exited
one and marked all rows **UNVERIFIED**, with no failed or incomplete
requirements: the build manifest explicitly recorded dirty plugin source,
and the standalone copied runner had no clean checkout identity.
The runs were uninstrumented; memory safety and Windows behavior remain
outside these observations.

Evidence remains under `~/server-state-audit.K1HeTV/` on the audit host.
It includes the selected matrix, plugin build manifest with source digests,
copied runner/catalog bundle, report, event logs, and rejected certification
evaluation. Isolated server copies were removed after execution; source
installations and older reports remain intact.

| Evidence | SHA-256 |
| --- | --- |
| Audit plugin | `3dc220c1f4a751ca538cae65c7561de1ab5ceca8e138d98480a3a8bfd072fe82` |
| Report | `7bddf52b1416b624d7aafdfd2254d1d6434e590b3cc2b0a348fbfa5cb0721b24` |
| Certification evaluation | `cc7034903675e3f33c8e790e815171ca18016659d888bf53e1cec2eb5c4b7c86` |
| Copied runner/catalog/plugin bundle | `6d3179fbf457ab188358a1c1ca0cf8aae160272251967a979d7b08ae516ccb43` |
| Valve/classic event log | `6e1f4e0e0a9912136da79fc592e412fdb14891a6aeaec6e67e0c67d79db2a475` |
| ReHLDS/classic event log | `5998e5c92f5ed5a1757bb2b69047666d957f0a46dedf271b0f12dcf7155e47a3` |
| ReHLDS/Metamod-R event log | `ede3d9bdc1d99952b777860e0dfa2f94f7933fa965118763f2db435b65fb41cc` |

Component hashes and console/mapping evidence are in the report.
The retained files have not been exported as a public offline-verifiable
bundle.

### Publish replayable certification evidence

From a checkout that supports the recorded certification profile, export
the live report with its per-runtime `events.jsonl` files still beside it
and the matrix used for the run.

```sh
python3 verify/audit/runner.py export-certification \
  /path/to/run/report/report.json /path/to/run/matrix.json core ./bundle
python3 verify/audit/runner.py verify-certification ./bundle
```

Export requires every row in the report to certify. It copies the original
event bytes, includes only the matrix rows covered by the report, and
projects the report onto the metadata used to replay policy. It substitutes
virtual staging paths in identity evidence and rechecks certification
after that substitution. The bundle includes a SHA-256 manifest, both
certificate formats, and the original source report hash for reference.
Its matrix records inputs and itinerary for review. It omits local launch
paths and is intended for offline verification.

Review the bundle before publishing. Raw server logs, binaries, launch
environments, and configuration files are excluded. The exporter rejects
unrecognized event fields and recognizable private data. Free-form event
text still requires human review. If it contains sensitive information,
withhold the bundle and recollect with public-safe inputs. Event bytes
must stay identical to those collected by the runner.

The verifier checks the inventory and hashes, revalidates the event
protocol, reruns the installed profile policy, and compares both resulting
certificates. Record the verifier checkout alongside the published bundle
so readers can use that policy version. A trusted publisher or separately
authenticated manifest is still needed to establish authenticity. Offline
replay cannot establish that captured process mappings or console
observations came from a real server. The archived November 22 certificate
is a summary. Its original event bundle is not included in the repository.

## Live Ricochet client admission results

On November 22, 2026, the engine demo was exercised with a real Ricochet
client on disposable copies of the saved Linux i686 servers. Both runs
used stock Ricochet on `rc_arena`.

| Runtime | Deferral | Rejection | Reconnect after rejection |
| --- | --- | --- | --- |
| Valve HLDS build 8684 with classic Metamod 1.21.1-am | Pass | Pass | Pass |
| ReHLDS 3.15.0.896-dev with Metamod-R 1.3.0.149 | Pass | Pass | Pass |
| Win32 | Pending | Pending | Pending |

The deferred connections joined and produced the demo's player-slot
greeting. The client named `RustAdmissionReject` received
`Rust admission demo rejected this name.` and produced no greeting.
An allowed client subsequently joined the same server instance that
rejected the test name. The disposable Valve server was restarted between
the initial deferral and the rejection/reconnect pair because the first
client session still occupied a slot.

These observations cover the engine demo on the two listed Linux
configurations. The manual runs are separate from core v2 certification
and synthetic ABI coverage. [Live test procedure](sdk-reference.md#test-both-outcomes-with-the-engine-demo)

## Console and manual protocol

Use a real local console if the runtime fails to receive piped stdin or redirect output. In particular, a Win32 engine may use console APIs that bypass redirected stdin/stdout. Prepare an isolated copy with `run ... --only ID --prepare-only`. Its `launch.json` contains the exact argv, working directory, and environment additions. Start the command in that directory using those values. Preparation writes a `runtime.json` and retains the staging copy for the operator. Keep that copy local and remove it when done. The prepare-only exit code describes staging, and the report state is `PREPARED`.

In the dedicated server console:

1. Wait for the `ready` record and run `version`, `meta version`, `meta list`, and `mm_audit_status`. Confirm the selected manager and plugin actually loaded.
2. Run `mm_audit_run all "audit two words"`. The optional quoted marker tests argument preservation. Individual suites are `console`, `entities`, `trace`, and `resources`.
3. Change to a second configured map with `changelevel MAP`. Wait for activation and run the suite again. Keep the callback sequence around the transition.
4. Connect a client, enter the match, disconnect, and reconnect. Record whether the same slot is reused. Repeat on the same map for the slot-reuse probe. Lag-compensation callbacks are optional and depend on the host/GameDLL and gameplay. Play normally and record their presence if they occur.
5. Run `mm_audit_report`, then `quit`. Preserve the console transcript through exit, including stderr if available. The launch enables `-condebug` for builds that support it. If the engine does not forward shutdown, or the platform does not expose cleanup logs, leave those observations skipped.

Import one run's transcript and optional structured operator findings into a new report directory:

```sh
python3 verify/audit/runner.py import prepared/ID/runtime.json imported/ console.log shutdown.log \
  --manual operator-findings.json
```

The manual file is a JSON array. Each object contains `test` (a test name or contract finding ID from the report), `result`, `classification`, `observed` with reproducible supporting evidence, and `operator`. Use one entry per identifier. A manual result can fill an unobserved probe. Conflicting evidence is labeled and does not erase an observed failure. For manually collected logs, include the observed process exit status in `runtime.json` when known. Keep the original transcripts referenced by the imported report. The protocol parser expects one plugin instance and one server run per import.

## Interpretation

The plugin emits one `MM_AUDIT `-prefixed, versioned JSON record per observation. The runner builds `events.jsonl` in sequence order, attaches runtime and run IDs, and keeps each test's contributing sequence numbers in `report.json`. `report.md` summarizes identity, execution and test evidence. Missing probes get a reason and remain SKIP. A prior observed FAIL survives retries. The plugin caps its output at 4,096 events including a terminal budget marker, tracks at most 64 disconnected slots, and records no auth identifiers or arbitrary client text. The resource suite performs 64 owned reads, reports time without asserting a latency threshold, and the runner samples process RSS where Linux exposes it. These snapshots can inform comparisons and do not establish a leak rate.

| Result | Meaning |
| --- | --- |
| PASS | The required scenario was observed and met its local expectation |
| FAIL | A concrete expectation failed under the exercised conditions |
| SKIP | Required callback, configuration, or scenario was unavailable or unobserved |
| INCONCLUSIVE | The scenario partially occurred, but the evidence was insufficient for an assertion |

Classification is independent of result. Automatic observations are scoped to the exact runtime and classified as **Implementation-specific behavior**. **Verified portable behavior** requires corroborating observations across the claimed platforms and versions. **Unverified host assumption** covers contracts that safe SDK observations cannot establish. Confirm a failure's cause before attributing an **SDK defect** or **Host defect**. Use **Unsupported configuration** for documented or evidenced exclusions, including the exact affected version range. A missing executable alone only establishes lack of coverage.

```sh
python3 verify/audit/runner.py compare report-dir/report.json
python3 verify/audit/runner.py compare previous/report.json current/report.json
```

One report compares its runtimes. Two reports compare stable runtime IDs across runs and distinguish PASS-to-FAIL regressions from coverage loss. Same-result differences in trace fractions and optional identity or lag evidence can be legitimate across maps and GameDLLs. To annotate known result transitions, pass `--allowances FILE` with a JSON array. Entries specify `test`, `before`, `after`, `reason`, and either `runtime` for historical comparison or `left` and `right` for a matrix comparison. Allowances annotate differences without changing the underlying results.

## FFI evidence and CI

| Contract | Live observation | Native fixture or other evidence |
| --- | --- | --- |
| Strings and registration | Private CVar registration and write/read, retained owned copy, exact command marker. Local input bounds are tested separately. | Retained versus copied storage needs implementation evidence. The native tests cover oversized input and reentrant getter invalidation. |
| Edict arena and serials | Map lifecycle, stale handle rejection, optional player slot reuse. | Tests cover retirement during lookup, serial replacement and trace invalidation. Raw arena lifetimes and serial arithmetic remain host contracts. |
| Hook results and reentrancy | Lag pre/post delivery and original result availability, with ignored return decisions. | C++ asserts fixture hook status and nested-delivery ordering. The safe plugin has no result-slot or native pointer inspection. |
| Shutdown and cleanup | Cleanup and destructor entry if `GameShutdown` is forwarded. | The native fixture asserts no later command dispatch, and existing tests cover shutdown during an FFI call and panicking cleanup. |

Run `python3 -m unittest discover -s verify/audit -v` for reporting and process fixtures. The ordinary repository CI builds and loads the audit plugin in the Linux i686 and Windows x86 ABI checks. Linux retains the loader ASan/UBSan/LSan run and the combined Rust/C++ ASan run. No Valve runtime is needed for these tests. Those checks exercise adversarial scenarios without using a real server as a fault-injection tool.

The optional **GoldSrc runtime audit** workflow is dispatch-only on a provisioned self-hosted runner with the `goldsrc-audit` label. Enable `GOLDSRC_AUDIT_ENABLED` and configure the applicable `GOLDSRC_AUDIT_LINUX_CONFIG` or `GOLDSRC_AUDIT_WINDOWS_CONFIG` path. Provision Git, Python, PowerShell, the target Rust toolchain, and the corresponding native build dependencies. The workflow uploads structured reports only. Keep raw logs and dumps on the authorized runner since diagnostics can contain local process data.

When adding a probe, use a stable `*.vN` identifier with an explicit expectation. Bump its identifier version if the expectation changes. Add the scenario to the native harness when safe public callbacks cannot establish it, and exercise its reporting path. Cross-runtime behavior tests and sanitizer runs can expose contract violations, but successful observations cannot prove that the host always supplies valid native pointers or that every interleaving is memory-safe.

## Runtime identity evidence

A report separates the operator's component labels, the SHA-256 of each staged file, and runtime corroboration. The runner asks for `version`, `meta version`, `meta list`, and `meta game` before checking the audit command round trip. It checks loaded files at that checkpoint with Linux process mappings (device and inode) or the Windows Toolhelp module snapshot (normalized module paths). Configure separate engine modules in `engine.modules` so verification checks the engine alongside its launcher.

For engine and Metamod identity, a PASS requires the expected version token and mapped artifacts. An unadorned numeric `version` is used as the expected token by default. If `version` is an operator release label or commit, set `console_version` to the exact token shown by the component. The runner compares ReHLDS's own banner independently of its HLDS-compatible executable version. Set the engine's `console_version_field` to `exe build` to compare the build number in parentheses. Metamod family matching recognizes the classic and Metamod-R banners. Binary provenance beyond those observations remains an operator claim.

A GameDLL is corroborated by the expected mapped file or the matching absolute path from `meta game` with a working audit command. A relative path alone is insufficient. The plugin requires a status reply and either a loaded-file match or a running entry in `meta list`. Missing output or inaccessible process metadata can yield INCONCLUSIVE. A recognizable contradictory version, path, implementation, or module snapshot is an identity failure. A clean process exit does not resolve either status. Transcripts imported manually may lack the module snapshots needed for identity verification.

The staged `liblist.gam` entry points to Metamod. Both managers support the localinfo overrides for the config file, plugin list, and GameDLL. The runner preserves the mod-relative GameDLL override and validates the files in the isolated copy. The overrides are staging inputs, and runtime corroboration is checked independently. [Classic Metamod initialization](https://github.com/alliedmodders/metamod-hl1/blob/master/metamod/metamod.cpp) · [Metamod-R initialization](https://github.com/rehlds/Metamod-R/blob/master/metamod/src/metamod.cpp) · [ReHLDS version command](https://github.com/rehlds/ReHLDS/blob/master/rehlds/engine/host_cmd.cpp#L2443-L2455)

## Repeated lifecycles and comparison

Set `map_cycles` to an integer from 1 through 8 to repeat the configured itinerary, with at most 64 map visits. Each transition records the requested map, correlated activation and deactivation sequence numbers, and any stale-handle checks. A callback confirms a lifecycle event. The requested map name is an input, so the report does not treat the callback as independent verification of that name. Compare RSS before changes, after each suite, and before quit. Historical comparison surfaces changes of at least 4 MiB in the beginning-to-end RSS delta as observations. Allocator behavior and map assets can affect RSS, so this threshold is neither a leak detector nor a pass criterion.

For reproducible player-slot observations, run one loopback server and one client with no bots or other clients. Join and enter the match, disconnect, then reconnect before a map transition. Check the recorded slot number on both joins and repeat if it changes. Record the exact map, client build, and commands in supporting evidence. Unavailable player interaction remains SKIP.

Historical comparisons track process exits, shutdown method, protocol loss, sanitizer and crash diagnostics, capture faults, runtime identities, and component hashes. Health regressions are reported even if all probe results are unchanged. Host artifact changes flag configuration drift and qualify probe comparisons. Audit plugin changes are shown separately so testing an SDK revision can still expose a regression on the same host. Every allowance needs a nonempty reason. It annotates a probe transition, preserves the underlying result and evidence, and does not suppress run-health regressions.

## Native fixture evidence

The C++ loader emits `MM_AUDIT_NATIVE` records after selected assertions complete. These carry stable `native.*.vN` identifiers for bounded command snapshots, hook-result behavior, reentrant entity operations, deferred delivery, and shutdown dispatch. A crash can leave only a partial transcript. Record the loader's actual exit status in the runtime JSON and import the complete transcript with `runner.py import ... --native-fixture`. The fixture rows are stored as `native_fixture_results`, and plugin callbacks observed by that loader are scoped as synthetic as well. They do not satisfy the identity checks or compatibility probes for a real server. The native loader's adversarial checks exercise SDK handling under controlled FFI conditions. Live-server observations leave pointer readability, unseen native reentry, registration ownership, arena retirement, and hook-result storage lifetime as host assumptions.

## First live baseline on Linux i686

1. Use a disposable, locally provisioned Valve HLDS tree with stock Ricochet and clean test configuration. Install the classic Metamod x86 binary in that tree. Keep the stock GameDLL in its own location. Confirm the launcher, engine module, Metamod, and GameDLL are ELF32 i386 files with `file` or `readelf -h`. Confirm the 32-bit loader and shared dependencies are available.
2. Copy [the single-row example](../verify/audit/linux-hlds-classic-stock.example.json) outside the installation. Replace the installation path, placeholders, and filenames with the actual files. Check `version` and `meta version` on your provisioned build. For split engines, list the actual loaded engine module in `engine.modules`. The sample uses the numeric HLDS build from `Exe build` and the release token from `meta version`. Record the source build of the GameDLL independently and pin SHA-256 values if you need repeatable artifact selection.
3. Run the fixture verification and the one selected live row from the repository root. Use new output directories outside the installation.

   ```sh
   bash verify/audit/collect-native.sh ./target/audit-native-first linux-i686-local
   python3 verify/audit/runner.py run /path/to/local/linux-baseline.json ./target/audit-live-first \
     --only linux-hlds-classic-stock --build
   ```

4. Check `report.json` for four corroborated runtime identities, the expected hashes, complete protocol, and observed deactivation and activation for the repeated map itinerary. Review `stdout.log` and `stderr.log` for real command responses and shutdown observations. If console transport fails, use `--prepare-only` and the manual console procedure. Treat unavailable module evidence or absent callbacks as a coverage gap. Run with `--client-wait` to exercise the documented join, disconnect, and same-slot reconnect sequence.
5. Repeat with the same files for a historical baseline. To expand coverage, add a distinct row for ReHLDS with classic Metamod, then ReHLDS with Metamod-R. Configure versions and paths from each test installation. Run separate stock and `ricochet-updated` rows with their own GameDLL binaries and hashes. The template also includes experimental HLDS with Metamod-R and the Windows x86 counterparts. Absence of a runnable row leaves that combination unverified.

The [console fixture provenance](../verify/audit/fixtures/README.md) distinguishes a third-party HLDS excerpt from transcripts constructed from upstream ReHLDS and Metamod source. Those inputs exercise parsing, including truncated classic plugin-list columns and Win32 path spelling. They are separate from live compatibility results.

## CI evidence and comparison exit status

Hosted Linux ABI jobs archive their loader and combined sanitizer transcripts with the JSONL and aggregate reports. Windows x86 archives the equivalent ABI and loader transcript and runs the Toolhelp test with the installed Python. The strict importer requires all six expected native probe IDs exactly once, a complete audit-plugin command protocol, and both shutdown records. Loader failures and incomplete evidence remain visible even when some assertions completed. The native evidence jobs use the local C++ loader and require no GoldSrc installation.

Comparison JSON separates **severity**, **comparability**, and **attribution**. Historical failure or coverage loss exits with status 1, including a newly failing probe across host configuration drift. An allowance records the expected difference and its rationale. The original result, evidence, and severity still affect the exit status. Host drift limits attribution and cannot establish an SDK regression. Matrix comparisons also surface each failing row independently of row order. Invalid comparison input exits with status 2.

### Diagnosing the first six-row Ricochet run

These diagnostics describe the initial failed run. The later
[Ricochet core v2 certificate](certifications/ricochet-ubuntu-core-v2/20261122T115759Z-6890/certificate.md)
records all six rows passing after the staging and registration fixes.

The dump collector uses filenames only to find candidates. It checks for
an ELF core header or a minidump header with an in-bounds stream directory
before copying. Exit codes and crash diagnostics remain independent
evidence, including on hosts that store dumps outside the stage.

The collector fix is Python-only. Keep using the original i686 audit plugin
and its original build manifest. The SHA-256 identifies the binary but
cannot recover its build commit. If the original build record is missing,
leave provenance unknown until you can recover it or build a new artifact
from a known clean checkout. Keep separate audit-plugin and SDK revisions
when the build used different revisions.

On the test machine, once the fix is available on GitHub, use a **separate
checkout** to run a fresh audit with the previously provisioned matrix.
Set `OLD` to the saved run that contains the six rows. Keep its matrix,
artifact, source metadata, and report in place.

```bash
git clone https://github.com/MiguelX413/metamod-sdk-rs.git "$HOME/metamod-sdk-rs-audit-fix"
SDK="$HOME/metamod-sdk-rs-audit-fix"
OLD="$HOME/ricochet-audit-lab/runs/ACTUAL_RUN_ID"
NEW="$(mktemp -d "$HOME/ricochet-audit-followup.XXXXXX")"

file "$OLD/libmetamod_audit.so"
sha256sum "$OLD/libmetamod_audit.so"
# Compare this hash with the saved matrix/build manifest before continuing.
# The first six-row run used cd510851d4ec735b5434a5ae911b1ff41e78a70338fa4d0a80badb3a00b79714.

python3 "$SDK/verify/audit/runner.py" run "$OLD/matrix.json" "$NEW/classic-rehlds" \
  --platform linux --only linux-rehlds-classic-stock --only linux-rehlds-classic-updated
python3 "$SDK/verify/audit/runner.py" certify "$NEW/classic-rehlds/report.json" \
  core "$NEW/classic-rehlds-cert"
```

Run all six rows by dropping the two `--only` flags and using another
output directory. This makes new isolated copies from the saved source
installations, using the configured prebuilt plugin. It requires neither
Rust nor a C++ toolchain on the test machine. The bootstrap's `--rerun`
is for replaying the saved runner revision and plugin. It does not switch
a saved run to the fixed runner. If you reprovision instead, use a fresh
bootstrap invocation with `--repo-ref` set to the fixed runner revision,
`--skip-steam` to reuse the Steam cache, and `--plugin` and
`--plugin-provenance` pointing to the actual build artifacts.

#### Capture the two startup failures

Start with the stock row for each pair. Prepare new disposable copies
using the same matrix and artifact. The runner retains the staged files
and records `launch.json` for each row.

```bash
python3 "$SDK/verify/audit/runner.py" run "$OLD/matrix.json" "$NEW/diagnostic" \
  --platform linux --prepare-only \
  --only linux-hlds-classic-stock --only linux-rehlds-metamod-r-stock
```

Use the following launcher to preserve the recorded working directory,
arguments and environment under `strace` and GDB. It removes the
preparation-only `-condebug` flag to match the automated launch. Install those diagnostic
tools if needed. Run from the LXC test machine. If GDB reports that ptrace
is blocked, collect its error as well and rerun on a test host that allows
debugging child processes.

```bash
cat >"$NEW/diagnose.py" <<'PY'
import json
import os
from pathlib import Path
import sys

launch_path, mode = sys.argv[1:]
launch = json.loads(Path(launch_path).read_text())
argv = launch["argv"]
if argv[2:3] == ["-condebug"]:
    del argv[2]
os.chdir(launch["cwd"])
env = {**os.environ, **launch["environment"]}
if mode == "trace":
    prefix = ["timeout", "--signal=TERM", "--kill-after=3s", "25s",
              "strace", "-f", "-s", "256", "-e", "trace=execve,openat,readlink,statx"]
elif mode == "gdb":
    prefix = ["timeout", "--signal=TERM", "--kill-after=3s", "60s", "gdb", "-q", "-nx", "-batch",
              "-ex", "set pagination off", "-ex", "set disable-randomization off",
              "-ex", "run", "-ex", "thread apply all bt full", "-ex", "info registers",
              "-ex", "info sharedlibrary", "-ex", "info proc mappings",
              "-ex", "x/12i $pc", "--args"]
else:
    raise SystemExit(f"unknown diagnostic mode: {mode}")
os.execvpe(prefix[0], prefix + argv, env)
PY

VALVE="$NEW/diagnostic/linux-hlds-classic-stock"
MMR="$NEW/diagnostic/linux-rehlds-metamod-r-stock"
python3 "$NEW/diagnose.py" "$VALVE/launch.json" trace >"$NEW/valve-open-trace.log" 2>&1
python3 "$NEW/diagnose.py" "$MMR/launch.json" gdb >"$NEW/metamod-r-gdb.log" 2>&1
```

For Valve HLDS, return `launch.json`, the original stdout/stderr, the open
trace, and the staged `server/ricochet/liblist.gam` and
configuration file named by `mm_configfile` in `launch.json`. The open trace should
show which listing the engine opened and whether it tried loading the
staged Metamod library before reporting `controller.cfg`. This distinguishes
engine-side library selection from later Metamod GameDLL resolution.

For Metamod-R, return `launch.json`, the original stdout/stderr, the GDB log
including the faulting PC and shared-library mappings, and the original
runtime metadata with engine, Metamod, GameDLL, and plugin hashes. The last
`map.activate.v1` event does not prove that the first frame started.
If the stack reaches the first-frame hook, the SDK's first engine calls
there are a CVar lookup, registration and read-back, followed by the three
command registrations. A frame in one of these paths would narrow the
cause. Keep the stock and updated logs separate if their stacks differ.

### Retesting Metamod-R CVar registration

The pinned Metamod-R registration wrapper identifies the plugin from the
address of the submitted `cvar_t`. A heap record does not normally belong
to a loaded ELF module. In revision `603a257`, both Linux
`CSysModule::find()` and `CSysModule::load()` may inspect uninitialized
`Dl_info` fields after a failed `dladdr()` and then use an invalid filename
with `dlopen()`. The minimal upstream correction is to return on lookup
failure before inspecting the output, using
`!dladdr(addr, &dlinfo) || !dlinfo.dli_fbase || !dlinfo.dli_fname`.
Command registration uses the callback address for ownership, and the
SDK already passes a module-resident callback.
[Metamod-R registration](https://github.com/rehlds/Metamod-R/blob/603a2574e937171aa5e737bd5e7b4a07f74e88ee/metamod/src/reg_support.cpp#L25-L69) ·
[Linux module lookup](https://github.com/rehlds/Metamod-R/blob/603a2574e937171aa5e737bd5e7b4a07f74e88ee/metamod/src/sys_module.cpp#L119-L144)

The SDK now supplies a writable record from the plugin image. The engine
may retain it and replace its string pointer. Each reservation consumes a
slot for the lifetime of the attachment, including rejected registrations.
Name and default-value storage also stay resident. The Linux loader
fixture verifies module ownership with `dladdr()`. It does not run
Metamod-R or establish the outcome on a live server.

**Build on the development machine.** After committing the fix and from a
clean checkout of that commit, run:

```bash
(
  set -euo pipefail
  test -z "$(git status --porcelain)" || { echo "Build from a clean checkout." >&2; exit 1; }
  revision="$(git rev-parse HEAD)"
  rustup target add i686-unknown-linux-gnu
  cargo build --locked --release --target i686-unknown-linux-gnu -p metamod-audit
  binary=target/i686-unknown-linux-gnu/release/libmetamod_audit.so
  file "$binary"
  nm -D --defined-only "$binary"
  hash="$(sha256sum "$binary" | cut -d' ' -f1)"
  printf '%s\n' "$hash"
  python3 - "$revision" "$hash" <<'PY' > target/i686-unknown-linux-gnu/release/audit-build.json
import json, sys
revision, digest = sys.argv[1:]
print(json.dumps({"schema_version": 1, "sha256": digest, "audit_plugin_commit": revision,
                  "sdk_commit": revision, "working_tree_dirty": False}, indent=2))
PY
)
```

Check that `file` reports a 32-bit i386 ELF shared object and `nm` lists
`GiveFnptrsToDll`, `Meta_Query`, `Meta_Attach` and `Meta_Detach`. Both
revision fields are equal here because this workspace build compiles the
SDK and plugin from the same checkout.

**Transfer and run on Ubuntu.** Transfer the new `libmetamod_audit.so` and
`audit-build.json` together through your normal transfer channel. Use a
clean runner checkout and a previously provisioned six-row matrix to avoid
downloading game assets again. Adjust the first three paths below.

```bash
SDK="$HOME/metamod-sdk-rs-audit-fix"
OLD="$HOME/ricochet-audit-lab/runs/ACTUAL_RUN_ID"
ARTIFACTS="$HOME/incoming/metamod-audit"
NEW="$(mktemp -d "$HOME/ricochet-cvar-followup.XXXXXX")"

python3 - "$OLD/matrix.json" "$ARTIFACTS" "$NEW/matrix.json" <<'PY'
import hashlib, json, sys
from pathlib import Path
old, artifacts, output = map(Path, sys.argv[1:])
plugin = (artifacts / "libmetamod_audit.so").resolve(strict=True)
manifest = json.loads((artifacts / "audit-build.json").read_text())
digest = hashlib.sha256(plugin.read_bytes()).hexdigest()
if digest != manifest["sha256"]:
    raise SystemExit("Transferred audit plugin does not match the build manifest")
matrix = json.loads(old.read_text())
for row in matrix["runtimes"]:
    row["installation"] = str((old.parent / row["installation"]).resolve(strict=True))
    row.update(plugin=str(plugin), plugin_sha256=digest, plugin_provenance=manifest)
    for field in ("audit_plugin_commit", "sdk_commit"):
        row.pop(field, None)
output.write_text(json.dumps(matrix, indent=2) + "\n")
print(digest)
PY

file "$ARTIFACTS/libmetamod_audit.so"
sha256sum "$ARTIFACTS/libmetamod_audit.so"
# Confirm that the checksum matches the development build's printed SHA-256.

run_core() {
  local output="$1"
  shift
  python3 "$SDK/verify/audit/runner.py" run "$NEW/matrix.json" "$NEW/$output" --platform linux "$@" &&
    python3 "$SDK/verify/audit/runner.py" certify "$NEW/$output/report.json" core "$NEW/$output-cert"
}
run_core stock --only linux-rehlds-metamod-r-stock
# After stock certifies:
run_core updated --only linux-rehlds-metamod-r-updated
# After the focused runs, run the whole saved matrix:
run_core full
```

Each invocation uses fresh isolated staging and keeps earlier evidence.
The runner checkout may differ from the artifact build checkout. Do not
use bootstrap `--rerun` to substitute the new binary into an old run.
Ubuntu only needs the prebuilt binaries, Python and runtime dependencies
for these commands. If the stock row still faults before readiness,
collect the GDB evidence described above for the new artifact.

### Retesting the two Valve HLDS configurations

On Linux Valve HLDS rows, the runner puts its configuration and plugin
list under `addons/mm_audit/` so legacy `+localinfo` parsing can read
their full paths. When classic Metamod is configured as
`metamod_i386.so`, the runner stages a byte-identical `metamod.so` copy
beside it, selects that path in `liblist.gam`, and verifies the hash and
process mapping of the actual loaded path. The plugin itself still lives
at `addons/metamod-audit/libmetamod_audit.so` and uses the saved SHA-256
and build provenance. ReHLDS keeps its existing staging layout.

Use a clean checkout containing the runner fix and the original six-row
matrix. These commands create a fresh run from the saved installations
and prebuilt plugin. No rebuild is needed.

```bash
SDK="$HOME/metamod-sdk-rs-audit-fix"
OLD="$HOME/ricochet-audit-lab/runs/ACTUAL_RUN_ID"
OUT="$(mktemp -d "$HOME/ricochet-valve-retest.XXXXXX")"

python3 "$SDK/verify/audit/runner.py" run "$OLD/matrix.json" "$OUT/audit" \
  --platform linux --only linux-hlds-classic-stock --only linux-hlds-classic-updated

python3 "$SDK/verify/audit/runner.py" certify "$OUT/audit/report.json" core "$OUT/certification"
```

Check both rows in `certificate.md` for **CERTIFIED** under core v2.
Keep the two commands separate so a failed audit can still produce a
certification evaluation. The bootstrap's `--rerun` keeps its pinned
runner revision, so use the direct command above to test a new revision.
