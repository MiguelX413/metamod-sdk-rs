# Release roadmap

Before a stable release, we need to choose the supported API and hosts, finish
native validation, and make distribution reproducible. The feature backlog
below can ship in later releases. The [API and host matrix](support-matrix.md)
proposes the initial scope; release dates remain open.

## Development priorities

Focus on game-agnostic SDK features and their validation:

1. Close validation gaps in existing generic APIs, including Win32 behavior
   and the remaining memory-contract scenarios.
2. Add small, useful engine capabilities with enforceable lifetime and phase
   rules. Owned entity relationship snapshots have synthetic coverage and
   [Linux live results](entity-relationships.md); Win32 remains unverified.
   Loading-only model/sound precaching has [Linux results](resource-precaching.md#live-linux-results)
   with classic Metamod and ReHLDS/Metamod-R; Win32 validation remains open.
   Resource lookup, generic downloads and event precaching remain raw.
3. Expand typed hooks as those capabilities need them. Keep ReHLDS extensions
   optional and retain portable behavior where the native interface permits it.

Further Ricochet-specific features are deferred. Maintain the existing
experimental adapter and its tests, including safety fixes. Generic fake-client
infrastructure must not depend on Ricochet private structures or setup rules.

Ricochet remains useful as a live test environment for generic features.
Those results establish behavior on the tested GameDLL, not other games.
Additional adapters and game logic follow the generic SDK work; they are not
prerequisites for its release.

## Current baseline

| Area | Implemented or recorded | Remaining boundary |
| --- | --- | --- |
| Plugin runtime | Startup registration, stateful callbacks, typed hook outcomes, panic containment, deferred owned notifications, shutdown handling | Preserve server-thread ownership, native reentry rules, startup-only loading, `PT_NEVER`, and detach refusal. |
| Engine services | Console/log output, commands, CVars, owned server timing/game-state snapshots, player sessions and information, entity discovery and snapshots, line/hull traces | General entity mutation, movement, networking, sound, events, and resource services remain largely raw. |
| Fake clients | Scoped identity, queued removal, separate stock Ricochet adapter; recorded Linux setup/reentry/reuse results | Other GameDLL setup contracts and Win32 live behavior remain unverified. |
| User messages | Bounded owned payloads, registration and recipient validation, conditional unsafe reliable unicast | Exact TextMsg/ScreenFade delivery is recorded for three Linux deployments. Win32 delivery and arbitrary schemas remain unverified. |
| Compatibility | Historical core v2 certificate for six identified Linux Ricochet configurations | This does not certify every current SDK feature, a later build, or Windows. |
| Memory evidence | Rust mocks/Miri, native adversarial probes, sanitizer tooling, Linux run/build binding | Remaining memory-contract probes and assessment policy; Windows provenance binding; live instrumented evidence. |
| Documentation | SDK reference, investigations, Rustdoc examples, missing-docs enforcement, HTML artifact/Pages workflow | Release support policy, migration guidance, and packaged documentation/build verification. |
| Distribution | Workspace builds from initialized pinned submodules | Public crates have `publish = false`; registry packaging and docs.rs builds are unresolved. |

The [coverage matrix](api-coverage.md) is the interface inventory. The
[runtime audit](runtime-audit.md), [memory audit](memory-safety-audit.md),
[fake-client investigation](fake-client-lifecycle.md), and
[messaging investigation](user-message-reentry.md) retain the detailed evidence.
This roadmap does not change their verdicts.

## First stable release milestones

### 1 Define the supported release

- [ ] Choose the initial stable APIs, which optional features or adapters remain
  experimental, and whether distribution initially uses Git tags, crates.io,
  or both. Preserve `metamod-sdk-rs`, `metamod_sdk_rs`, `metamod-sys`, and
  `metamod-macros` names.
- [ ] Declare native targets, minimum Rust version, Linux libc baseline,
  Windows runtime/toolchain requirements, and supported engine/manager/GameDLL
  rows. Linux i686 and Windows x86 are the existing build targets; a target
  becomes supported for a feature only after its required validation.
- [ ] Publish feature-by-host support labels: validated, experimental,
  unverified, or unsupported, with the exact evidence behind each claim.
- [ ] Define API stability, upgrade and deprecation policy, panic behavior,
  host obligations, and intentionally unsupported operations.

**Ready when:** the release API and supported hosts are listed.
A failing promised row must be fixed or explicitly removed from that scope,
with a reason, rather than relabeled as passing.

### 2 Complete Memory Contracts Validation v1

The initial string-invalidation, retired-edict, and retained-CVar probes and
Linux build binding already exist. Complete the coverage audit before adding
probes: existing tests may cover part of a scenario but need stronger assertions
or a separately required versioned record.

- [ ] Map each contract to Rust/Miri tests, native probes, and remaining host
  obligations. Cover same-map edict serial reuse as well as map retirement;
  trace hit/skip invalidation; nested shutdown while native code borrows Rust
  strings; and outer hook-result storage across reentry and shutdown.
- [ ] For each uncovered case, add an adversarial native probe that emits PASS
  only after assertions finish. Require its versioned result in strict imports
  and reject absent, duplicate, conflicting, malformed, and unexpected records.
- [ ] Add Windows run/build identity binding, retaining explicit UNVERIFIED
  provenance until its collector and validator are implemented. Exercise
  changed artifacts, mixed transcripts, target mismatches, and failed runs.
- [ ] Run the expanded fixtures under Linux C++ ASan/UBSan/LSan, combined Rust
  and C++ ASan, and the selected Miri scenarios. Retain native Windows ABI and
  loader checks; record unavailable Windows instrumentation explicitly.
- [ ] Define the versioned memory-contract assessment with separate verdicts
  for synthetic SDK validation, observed behavior on identified live hosts,
  and externally supported host contracts. Cover pointer readability, returned
  strings, registration ownership, edict arenas, result slots, and callback
  code lifetime. Reuse the existing memory evidence tooling where appropriate.
- [ ] Document reproduction and rejection cases. Preserve the core/player/
  extended v2 compatibility profiles and historical certificates unchanged.

**Ready when:** every in-scope contract has traceable evidence and an explicit
remaining obligation. Synthetic success does not become a blanket Rust
soundness verdict; incomplete, inconclusive, and unsupported results remain
visible. A contract required by a promised safe API needs a defensible host
boundary, or that API's scope must change.

### 3 Validate the supported native deployments

These runs need isolated installations and consenting clients where client
behavior is involved. Retain binaries, build provenance, component revisions
and hashes, complete logs, payload evidence, and normal server exit status.

- [ ] Rerun ABI, loader, and feature regressions on a clean release candidate
  for Linux i686 and Windows x86. Historical success is retained as historical
  evidence, not assigned to the candidate build.
- [ ] Run the declared live compatibility matrix on both operating systems,
  including both Metamod implementations wherever claimed. Keep optional
  GameDLL variants and experimental engine/manager combinations distinct.
- [ ] Finish Win32 client admission: deferral, rejection, bounded reason output,
  reconnect, and session identity. Exercise the promised Linux rows again for
  the release candidate.
- [ ] If an adapter is included in the release scope, finish its Win32
  fake-client setup, full-slot/allocation failure, partial setup,
  queued removal confirmation, reuse, pending removal across a map change,
  and reentry. Cover every adapter/host pair included in the release scope.
- [ ] Finish Win32 scoped message delivery with a real client and exact payload
  checks. Repeat the selected Linux protocols for the release candidate;
  preserve the unsafe host opt-in and record which native plugins/hooks exist.
- [ ] Collect live native memory evidence on the selected deployments with
  instrumentation suitable for that platform. Start with one instrumented
  Linux row before expanding. Attribute findings to the actual component and
  keep instrumentation failures separate from clean runs.
- [ ] Exercise repeated map changes, connect/disconnect cycles, entity reuse,
  command/CVar use, and shutdown. Define and record the load, duration, and
  resource-growth acceptance criteria before running endurance tests.

**Ready when:** every release support claim has an identified passing run or an
explicit feature exclusion. Compatibility, Rust-contract assessment, and
native memory results remain independent. Windows ABI success alone does not
satisfy a Windows live-delivery claim.

### 4 Make distribution reproducible

The current `metamod-sys` build expects `../upstream` beside its package.
An extracted crates.io package does not automatically contain that workspace
layout or initialized Git submodules. Solving this is a release engineering
requirement if registry publication is selected.

- [ ] Establish a distribution layout that builds from the actual shipped
  sources with the pinned upstream inputs and required licenses. Keep bindgen
  grounded in pinned upstream headers; do not hand-copy ABI definitions.
- [ ] For registry distribution, make the SDK, raw, and macro dependency graph
  publishable in dependency order with matching version requirements. Decide
  separately whether any adapter is part of that release; Ricochet adapter
  publication is deferred under the current priorities.
- [ ] Build extracted package contents on Linux and Windows without borrowing
  files from the development checkout. Verify manual registration with default
  features disabled and attribute registration with defaults enabled.
- [ ] Verify the minimum Rust version and declared system prerequisites,
  including libclang and the target C/C++ headers/linker. Derive the Linux
  libc claim from the shipped artifacts and a representative baseline host.
- [ ] Verify documentation generation from the distributed form. For crates.io,
  configure and test the intended docs.rs target and build requirements before
  publication; GitHub Pages remains a preview/distribution option.
- [ ] Review license texts, upstream notices, package contents, artifact names,
  reproducible build instructions, and release checksums. Remove
  `publish = false` only when registry publication is actually intended and
  the package checks pass.

**Ready when:** a new checkout or extracted distribution can build hello-world,
engine-demo, and player-tracker using only the documented inputs. The chosen
publication route has no hidden dependency on the maintainer's workspace.

### 5 Freeze and release the supported API

- [ ] Review public names, errors, defaults, hook opt-ins, feature flags,
  lifetimes, and unsafe contracts. Resolve breaking changes before the stable
  API promise. Check that generic SDK APIs stay game-agnostic.
- [ ] Demonstrate each promised feature through a compiled example and an
  applicable native test. Keep setup, install, error handling, map changes,
  reentry, and shutdown guidance consistent with actual behavior.
- [ ] Document migration from the preceding development API, known limitations,
  the support matrix, and how to report a reproducible native issue.
- [ ] Require the release's CI checks: formatting, workspace check/Clippy/tests,
  both registration paths, Rustdoc/doctests, Python evidence-tool tests,
  Linux native/sanitizer jobs, Miri, and Windows ABI/loader validation.
- [ ] Review a clean tagged candidate, archive its evidence and artifact hashes,
  then publish through the selected route. Verify that users can consume the
  published form and that hosted documentation matches the released version.

**Ready when:** the release defined in milestone 1 is available, reproducible,
documented, and backed by the required evidence. Maintenance continues for regressions,
security findings, upstream compatibility, and toolchain changes.

## Broader SDK feature milestones

These families extend the first stable release. Promote a family into the
release scope only with its validation work; implementing a wrapper alone does
not complete the family. They can also ship as later versioned releases.
The table is an inventory, not execution order; the development priorities above
put generic engine services and hooks ahead of game-specific adapters.

| Family | Feature work | Required validation before support |
| --- | --- | --- |
| Typed GameDLL hooks | Add missing lifecycle/gameplay observations and synchronous decisions, including spawn/think/use/touch where suitable. | ABI layouts, owned/deferred input handling, original-result semantics, nested invocation, panic behavior, and live dispatch on each promised GameDLL. |
| NewDLL and CVar replies | Private-data release notifications, collision decisions, and correlated CVar query/reply handling. | Pointer retirement timing, query/session identity, missing or late replies, reentry, disconnect, and shutdown. |
| Entity operations | Generation-checked creation/removal, narrowly scoped mutation, and supported GameDLL entity dispatch. | Same-slot reuse, map retirement, callbacks during creation/removal, private-data ownership, and GameDLL-specific invariants. |
| Events and sound | Typed parameters and wrappers for emitting/stopping sound and firing native events. | Argument encoding, resource lifetimes, phase restrictions, recipient/session changes, and real-client behavior. |
| Resource services | Model/sound precaching and typed indices are implemented with static names. Extend to event scripts, generic downloads and lookup only after their contracts are reviewed. | Linux classic Metamod and ReHLDS/Metamod-R observations exist; Win32 and clean-candidate validation remain open. Preserve initialization-phase restrictions, map-scoped index validity, and native retention/failure contracts. |
| Additional messaging | Schemas/adapters, destination modes, and optional host integrations justified by ownership guarantees. | Exact client bytes and recipients, payload limits, hook compatibility, lifecycle interruption, and explicit exclusivity contracts. Portable unrestricted safe sending is not assumed possible. |
| Movement and physics | Audit opaque payload layouts, then design callback-scoped reads/mutation and selected movement services. | x86 layout checks on both platforms, aliasing and reentry analysis, mutation boundaries, retained-pointer rejection, and gameplay regressions. |
| Networking and client state | Scoped snapshots or hooks for entity/client state, user commands, and packet construction where valid. | Per-invocation lifetimes, writable output bounds, protocol consistency, session replacement, and live client verification. |
| User info and save/restore | Bounded user-info operations and scoped serialization callbacks where supported. | Ownership, capacity, encoding, callback lifetime, map transitions, and save/load compatibility with the selected GameDLL. |
| Metamod utilities | Game information and audited utility calls beyond logging and message lookup. | String lifetime, phase, dispatch and reentry contracts. Plugin-control functions must preserve the resident-plugin policy. |
| Raw ABI coverage | Inventory transitively omitted or opaque public payloads by pinned header family and add needed ABI types. | Signature/layout comparison on Linux i686 and Windows x86, upstream provenance and licenses, and explicit exclusion of GameDLL implementation internals. |
| Additional adapters and games (deferred) | Resume game-specific setup/protocol adapters after the generic SDK priorities. Keep them separate from the SDK. | Dedicated game/host matrices and real client tests. Ricochet evidence cannot certify another game. Bot AI and game logic are separate projects or examples. |

**Broader completion:** every slot/type family in the pinned upstream inventory
has a disposition: supported wrapper, raw unsafe access, deferred with a concrete
blocker, or intentionally unsupported with a reason. Every feature claimed as
supported meets the common completion rule below. Deferred entries mean full
feature completion remains open, even when the first stable release is done.
New upstream revisions start a new inventory review.

## Completion rule for every feature

A feature is complete only when all applicable items below are satisfied:

1. Its host ownership, callback phase, thread, allocation lifetime, identity,
   native reentry, and failure contracts are explicit. Unsafe requirements are
   stated at the opt-in or operation that relies on them.
2. Its ABI shape is sourced from the pinned headers and checked on every
   promised native target. Raw pointer checks are not treated as proof of
   allocation validity.
3. Rust tests cover ordinary and boundary behavior; Miri covers the relevant
   Rust lifetime/aliasing paths. Native adversarial fixtures cover the native
   call boundary and reentry cases that mocks cannot establish alone.
4. Sanitizer/instrumentation scope is recorded accurately. Live acceptance
   runs establish the claimed behavior on identified hosts; client-facing
   features include client observations or payload evidence as appropriate.
5. Evidence is bound to its run and build, imports reject invalid/incomplete
   evidence, and a versioned assessment identifies the actual supported scope.
6. Documentation includes a working usage example, meaningful errors,
   lifecycle behavior, and limitations. CI checks the parts it can reproduce;
   externally provisioned runs retain their original artifacts and logs.

An item can be inapplicable with a reason; a Rust-only payload constructor does
not need a live engine to establish its byte encoding. Missing native evidence
cannot be excused this way for a native-delivery claim.
