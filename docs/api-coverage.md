# API coverage

The pinned headers in `upstream/` are the slot-by-slot inventory. This matrix
tracks SDK support by interface and safety contract. Update the relevant row
and verification when a wrapper is added. A generated table exposes its
function pointers through `metamod_sys::raw`; it does not imply that its
pointees have exposed fields or that the functions are safe to call.

| Interface and upstream inventory | Raw FFI | Safe SDK | Safety requirements and verification |
| --- | --- | --- | --- |
| Metamod loader and results, `metamod/meta_api.h`, `plinfo.h`, `h_export.h` | Entry point typedefs, metadata, result globals, and all eight table factory slots | Export macros, startup attachment, resident plugin policy, typed outcomes for synchronous hooks | x86 loader and layout probes. Loader handoff binds server-thread ownership. Mutable result slots belong to the current native invocation. |
| Metamod utilities, `metamod/mutil.h` | Utility table generated | Console and log output | Fixed formats for varargs. Game info, user-message lookup, entity dispatch, and plugin control still need individual ownership and reentrancy contracts. |
| GameDLL lifecycle and player callbacks, `engine/eiface.h` `DLL_FUNCTIONS`, `metamod/dllapi.h` | Complete callback table | Frame, map activate/deactivate, client admission pre, player put-in-server/disconnect, read-only keyvalue pre, and lag-compensation pre/post | Loader probes cover registrations, original lag-compensation return, and owned pre-activation and deferred keyvalue payloads. The native loader checks admission return values, result slots, and rejection buffers through the C ABI. Rust mocks also cover exact-length and split UTF-8 rejection text, panic containment, and shutdown during admission. User-info editing, save/restore, movement, entity dispatch, and networking hooks remain raw; pointer/out-buffer validity and synchronous results need individual treatment. |
| NewDLL, `engine/eiface.h` `NEW_DLL_FUNCTIONS` | Complete callback table | Optional game shutdown | Layout and loader probes. Entity-private-data release, collision filtering, and CVar replies remain raw. Their lifetimes and query correlation need separate contracts. |
| Engine console, commands and CVars, `engine/eiface.h` `enginefuncs_t`, `common/cvardef.h` | Engine slots and CVar layout generated | Native byte printing, validated console names for registration and dispatch, owned command tokens, owned CVar reads, validated borrowed CVar writes | Native harness exercises reentrant commands and CVar storage. Server command execution and arbitrary engine callback hooks remain raw. |
| Engine players and entities, `engine/eiface.h`, `engine/edict.h`, `engine/progdefs.h` | Callback slots and edict, entvars, and engine globals layouts | Session-aware player lookup, typed `PlayerUserId` and owned auth strings, index lookup, bounded edict discovery, owned spatial and classification snapshots, and retained string keyvalues | Native harness exercises session revalidation. All entity handles check the map epoch, edict serial, and free flag. Discovery and handle resolution require a validated map capacity from engine globals. The x86 probes cover vacant and out-of-range slots, spatial reads, and map transitions. The string-table probe checks signed offsets, reused buffers, and map invalidation. Entity creation/removal and GameDLL-private stored keyvalues remain raw. The keyvalue hook observes dispatched inputs, including keys consumed at load time. |
| Engine tracing, physics and movement, `engine/eiface.h`, `pm_shared/pm_defs.h` | Engine slots and trace payload layout; movement payloads remain opaque | Validated finite trace segments, owned line and standard hull results, collision mode and generation-checked skip entity | x86 layout/loader probes cover line hit, hull miss, skip entity, and a map transition during a trace. Movement and physics state still require callback-scoped mutation and reentry analysis. |
| Engine fake clients, messages, events, sound and resources, `engine/eiface.h` | Engine slots | Scoped fake-client identity and queued removal | Fake-client lifecycle is available through an explicitly opted-in GameDLL adapter. The stock Ricochet adapter is verified on two Linux i686 configurations. Message construction needs a complete, validated send operation around the native begin/write/end sequence. Resource registration is phase-dependent. |
| Other SDK data and auxiliary interfaces, `common/`, `engine/`, `pm_shared/`, `dlls/` | Selected transitively referenced types; no blanket header coverage | Pending | Add audited public ABI types by header family. Keep game implementation types and templates outside the ABI surface. |

The Linux i686 layout probes and loader harness passed for both examples.
Win32 CI is configured to run the same cases. The harness simulates the host.
The live Ricochet certificate covers the core v2 probes on the recorded
Linux i686 configurations. [Ricochet core v2 certificate](certifications/ricochet-ubuntu-core-v2/20261122T115759Z-6890/certificate.md)

The loader models nested map activation and checks that tracing stays
unavailable until the outer activation completes. It also supplies an
unterminated command token backed by the entire read budget. A Rust
discovery test covers a lookup that returns a retired arena pointer after
a map change. A shutdown regression checks that interrupted CVar lookups
return `EngineError::Unavailable`.

## Extension rules

- Use the bindgen callback table as the signature source. Keep native pointer
  reads and writes in the FFI boundary and add layout or loader checks for
  each new call shape.
- Keep observations deferable only when their payload is owned. A synchronous
  decision, writable buffer, or borrowed native handle needs a same-call
  policy. Returning hooks skipped under reentry leave `MRES_IGNORED` and
  return a placeholder the host ignores.
- Use `HookResult<T>` for pre decisions and `PostHookResult<T>` for post
  decisions. Superceding skips the GameDLL call and is a pre-only operation.
  Copy original returns before calling plugin code and read only storage the
  specific ABI callback supplies. Metamod owns result arbitration.
- Prefer snapshots and validated handles to references into the engine. Take
  copies before another native call and revalidate identities after calls
  that can reenter. Keep map, callback, and engine lifetimes distinct.

## Next milestones

1. Finish live client admission verification on Win32. Linux i686 passed
   deferral, rejection, and reconnect on Valve HLDS with classic Metamod
   and on ReHLDS with Metamod-R. [Live Ricochet results](runtime-audit.md#live-ricochet-client-admission-results)
2. Implement [fake-client lifecycle management](#fake-client-lifecycle-management).
3. Add validated message construction, then audit movement and physics and
   expand raw payload coverage family by family.
4. Build [Memory Contracts Validation v1](#memory-contracts-validation-v1)
   using the existing native fixtures, runtime reports, and certification
   infrastructure.

### Fake-client lifecycle management

1. **Establish host behavior.** Live Linux i686 fixture runs covered
   allocation, callback order, full slots, partial setup, and slot reuse
   on Valve HLDS and ReHLDS with classic Metamod. Stock Ricochet setup,
   queued teardown, and reuse also passed on both. Win32 and other
   GameDLLs remain open. [Investigation and live results](fake-client-lifecycle.md)
2. **Expose the supported lifecycle.** Implemented scoped creation through
   the separate stock Ricochet adapter, `PlayerId` tracking, and queued
   removal with explicit confirmation. [SDK and adapter results](fake-client-lifecycle.md#rust-sdk-and-ricochet-adapter)
3. **Verify the lifecycle.** Exercise creation failure, nested callbacks,
   disconnect, slot reuse, and map transitions with synthetic fixtures.
   Live Linux i686 tests passed on both engines for a map change with
   pending removal, same-slot reuse, and confirmed replacement removal.
   Both engines also passed forced reentry after native setup and before
   SDK session publication. Win32 remains pending. [Live map-change results](fake-client-lifecycle.md#live-map-change-with-a-pending-removal)

The first increment covers creation, identity, and supported teardown.
Movement, bot AI, and game-specific setup are later work.

### Memory Contracts Validation v1

1. **Exercise native lifetime boundaries.** Add versioned adversarial probes
   for string invalidation during reentry, edict reuse and arena retirement,
   trace invalidation, nested shutdown and hook-result storage, and retained
   CVar registration storage. Each probe must report its result after its
   assertions complete. Document which invalidations the SDK can observe and
   which lifetime guarantees the host must uphold.
2. **Make fixture evidence reproducible.** Extend the native importer to
   enforce the required probe set and reject missing, duplicate, malformed,
   or unexpected records. Bind results to the fixture run, build revisions,
   target, binary and log hashes, toolchain, and sanitizer configuration.
   Run the expanded suite in the existing Linux i686 sanitizer jobs and
   retain Win32 ABI coverage. Record unavailable instrumentation explicitly.
3. **Evaluate three dimensions independently.** Version the profile and
   report separate verdicts for synthetic SDK validation, observed behavior
   on identified live hosts, and host contracts supported by external
   evidence. Cover pointer readability, returned strings, registration
   ownership, edict arenas, hook-result storage, and callback code lifetime.
   Keep incomplete, unverified, and unsupported results visible. Preserve
   the existing compatibility profiles and historical certificates.
4. **Publish the reproduction workflow.** Document the probes, evidence
   requirements, commands, and example output. Test rejection of crashes,
   sanitizer findings, and mismatched builds or runs. A clean fixture
   run supports the invariants it exercised. Live behavior and host
   obligations require their own evidence.
