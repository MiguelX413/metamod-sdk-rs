# API coverage

The pinned headers in `upstream/` are the slot-by-slot inventory. This matrix
tracks SDK support by interface and safety contract. A generated table exposes its
function pointers through `metamod_sys::raw`; it does not imply that its
pointees have exposed fields or that the functions are safe to call.

| Interface and upstream inventory | Raw FFI | Safe SDK | Safety requirements and verification |
| --- | --- | --- | --- |
| Metamod loader and results, `metamod/meta_api.h`, `plinfo.h`, `h_export.h` | Entry point typedefs, metadata, result globals, and all eight table factory slots | Export macros, startup attachment, resident plugin policy, typed outcomes for synchronous hooks | x86 loader and layout probes. Loader handoff binds server-thread ownership. Mutable result slots belong to the current native invocation. |
| Metamod utilities, `metamod/mutil.h` | Utility table generated | Console and log output, user-message lookup | Fixed formats for varargs. Lookup validates native ID and size on an active map. Game info, entity dispatch, and plugin control still need individual ownership and reentrancy contracts. |
| GameDLL lifecycle and player callbacks, `engine/eiface.h` `DLL_FUNCTIONS`, `metamod/dllapi.h` | Complete callback table | Frame, map activate/deactivate, client admission pre, player put-in-server/disconnect, read-only keyvalue pre, and lag-compensation pre/post | Loader probes cover registrations, original lag-compensation return, and owned pre-activation and deferred keyvalue payloads. The native loader checks admission return values, result slots, and rejection buffers through the C ABI. Rust mocks also cover exact-length and split UTF-8 rejection text, panic containment, and shutdown during admission. User-info editing, save/restore, movement, entity dispatch, and networking hooks remain raw; pointer/out-buffer validity and synchronous results need individual treatment. |
| NewDLL, `engine/eiface.h` `NEW_DLL_FUNCTIONS` | Complete callback table | Optional game shutdown | Layout and loader probes. Entity-private-data release, collision filtering, and CVar replies remain raw. Their lifetimes and query correlation need separate contracts. |
| Engine console, commands and CVars, `engine/eiface.h` `enginefuncs_t`, `common/cvardef.h` | Engine slots and CVar layout generated | Native byte printing, validated console names for registration and dispatch, owned command tokens, owned CVar reads, validated borrowed CVar writes | Native harness exercises reentrant commands and CVar storage. Server command execution and arbitrary engine callback hooks remain raw. |
| Engine players and entities, `engine/eiface.h`, `engine/edict.h`, `engine/progdefs.h` | Callback slots and edict, entvars, and engine globals layouts | Session-aware player lookup, typed `PlayerUserId` and owned auth strings, index lookup, bounded edict discovery, owned spatial and classification snapshots, and retained string keyvalues | Native harness exercises session revalidation. All entity handles check the map epoch, edict serial, and free flag. Discovery and handle resolution require a validated map capacity from engine globals. The x86 probes cover vacant and out-of-range slots, spatial reads, and map transitions. The string-table probe checks signed offsets, reused buffers, and map invalidation. General entity creation/removal and GameDLL-private stored keyvalues remain raw; fake clients use the separate adapter lifecycle below. The keyvalue hook observes dispatched inputs, including keys consumed at load time. |
| Engine tracing, physics and movement, `engine/eiface.h`, `pm_shared/pm_defs.h` | Engine slots and trace payload layout; movement payloads remain opaque | Validated finite trace segments, owned line and standard hull results, collision mode and generation-checked skip entity | x86 layout/loader probes cover line hit, hull miss, skip entity, and a map transition during a trace. Movement and physics state still require callback-scoped mutation and reentry analysis. |
| Engine fake clients, messages, events, sound and resources, `engine/eiface.h` | Engine slots | Scoped fake-client identity and queued removal, owned user-message payload construction | The stock Ricochet adapter is verified on two Linux i686 configurations. Payload tests cover wire encoding, fixed-size checks, and the 192-byte budget. Live begin/write/end probes demonstrate shared native message state and reentry interference. Scoped reliable unicast requires explicit unsafe host opt-in; portable exclusive ownership cannot be enforced. Resource registration is phase-dependent. |
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
2. Finish Win32 verification for [fake-client lifecycle management](#fake-client-lifecycle-management).
3. Finish Win32 verification for [validated user messages](#validated-user-messages).
   Then audit movement and physics and expand
   raw payload coverage family by family.
4. Build [Memory Contracts Validation v1](#memory-contracts-validation-v1)
   using the existing native fixtures, runtime reports, and certification
   infrastructure.

### Fake-client lifecycle management

The separate stock Ricochet adapter provides scoped creation, `PlayerId`
tracking, and queued removal with explicit confirmation.
[SDK and adapter results](fake-client-lifecycle.md#rust-sdk-and-ricochet-adapter)

Linux i686 fixtures on Valve HLDS and ReHLDS with classic Metamod covered
allocation, callback order, full slots, partial setup, and slot reuse.
Stock Ricochet setup, queued teardown, and reuse passed on both engines.
Live tests also passed map changes with pending removal, same-slot reuse,
and confirmed replacement removal. Forced reentry during allocation with
the test GameDLL and after native setup with stock Ricochet published no
stale sessions.
[Investigation](fake-client-lifecycle.md) · [Map-change results](fake-client-lifecycle.md#live-map-change-with-a-pending-removal)

Win32 and other GameDLLs remain unverified. Synthetic fixtures should cover
creation failure, nested callbacks, disconnect, slot reuse, and map
transitions. Movement, bot AI, and further game-specific setup remain future work.

### Validated user messages

`UserMessagePayload` encodes bounded integer and native string fields without
native calls. Tests cover exact wire bytes, NUL accounting, fixed-size checks,
and failed writes that preserve the payload. Registration lookup checks the
native ID and size on the active map; recipient validation checks `PlayerId`
and current edict identity. The scoped sender repeats these checks at the
send boundary.

Native sending requires explicit unsafe host opt-in for each deployment.
It supports reliable unicast, rejects nested SDK sends, and stops native calls
on observed map changes or shutdown without automatically ending the message.
The portable ABI cannot enforce exclusive ownership: clientless Linux i686
probes on Valve HLDS and ReHLDS with classic Metamod showed shared message
state, nested begin/write/end interference, and plugin engine-table writes
bypassing classic Metamod observers.

Separate stock Ricochet client tests verified TextMsg and ScreenFade delivery
and exact client-demo payload bytes on three Linux i686 configurations:

- Valve HLDS with classic Metamod
- ReHLDS with classic Metamod
- ReHLDS with Metamod-R

These controlled deployments had no message-hook plugins. Client protocol,
host integration, and delivery requirements still need review for each
deployment. [Investigation and recorded results](user-message-reentry.md)

Win32 sending remains unverified. Run the ABI/loader and scoped-send regressions
on Windows, then check reliable delivery and exact client-demo payloads on
isolated stock Ricochet servers. Preserve component identities, artifact hashes,
and normal shutdown evidence as in the Linux runs.

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

### Public API Rustdoc audit

Audited on 2026-11-24 with `missing_docs` warnings enabled for
`metamod-sdk-rs`, with default features and with `--no-default-features`.
Both configurations reported **124 missing documentation comments**. This is a lint count, not a percentage of documented
API or an assessment of documentation quality. Private implementation details,
`#[doc(hidden)]` internals, generated raw bindings, and the separate macro and
adapter crates are outside this count. The macro's existing documentation is
available through its SDK re-export when default features are enabled.

| Source | Missing comments |
| --- | ---: |
| `trace.rs` | 54 |
| `message.rs` | 22 |
| `engine.rs` | 12 |
| `fake_client.rs` | 12 |
| `lib.rs` | 10 |
| `hooks.rs` | 10 |
| `runtime.rs` | 4 |
| **Total** | **124** |

By item kind: 52 enum variants, 43 fields, 16 methods, seven enums,
three structs, two associated constants, and one associated function.
All emitted intra-doc links resolve in both configurations when broken links
are treated as errors. The crate homepage examples are included in doctest verification.

Remaining work:

1. Document error variants and public data fields, including units, valid
   ranges, unavailable operations, and lifecycle invalidation semantics.
2. Fill descriptions of public types and methods, metadata fields, and hook
   opt-in constants. Explain defaults, ignored results, and reentry behavior
   where callers need them.
3. Review existing prose for accuracy and add task-oriented examples for engine
   services, entity identity, traces, and fake-client adapters. A satisfied
   `missing_docs` lint alone does not establish usable documentation or a
   complete unsafe contract.
4. Enable an enforced missing-docs policy once the intentional public surface
   is documented. Keep generated bindings and hidden implementation helpers
   separate from that policy. The Rustdoc workflow builds documentation and
   runs doctests for both feature configurations with warnings treated as errors;
   missing-docs enforcement remains deferred.

Reproduce from the repository root with the usual pinned submodules and
libclang prerequisites:

```bash
RUSTDOCFLAGS='-W missing-docs -D rustdoc::broken_intra_doc_links' \
  cargo doc --locked -p metamod-sdk-rs --no-deps
RUSTDOCFLAGS='-W missing-docs -D rustdoc::broken_intra_doc_links' \
  cargo doc --locked -p metamod-sdk-rs --no-deps --no-default-features
cargo test --locked -p metamod-sdk-rs --doc
cargo test --locked -p metamod-sdk-rs --no-default-features --doc
```

For local previews and hosted documentation, see the
[SDK reference](sdk-reference.md#rustdoc-previews).
