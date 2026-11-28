# API and host support

This is the proposed first stable-release scope, based on the recorded
evidence linked below. It is not a certification of the current
working tree or an API stability promise for the development version.
The [release roadmap](roadmap.md) lists the work required before release.

## API scope

| API family | Proposed release status | Boundary |
| --- | --- | --- |
| Plugin registration and runtime | Core | Attribute registration with default features; manual `Metadata` and `export_plugin!` with default features disabled. Startup loading only, server-thread state, deferred owned notifications, panic containment, and optional shutdown delivery. |
| Console, commands, and CVars | Core | Validated names and values, owned command tokens and CVar reads, bounded permanent registrations. Register once from an attached callback, not `initialize`. |
| Server timing and game state | Core API; candidate validation pending | Active-map owned snapshots, optional fields for invalid native values, and numeric GameDLL-defined modes. Linux development results are recorded below; Win32 validation remains pending. |
| Player identity and information | Core | Distinct slot, native user ID, and tracked session types; owned information snapshots. Tracking covers observed joins. Revalidate saved sessions. |
| Entity discovery and snapshots | Core API; candidate validation pending | Map epochs and native serials, bounded index lookup, owned common fields, strings and relationships. Relationship targets describe current observed occupants, not historical pointer intent. Relationship snapshots have synthetic coverage and recorded Linux results on three host combinations; Win32 and other combinations remain unverified. No general entity mutation or GameDLL-private data access. |
| Line and standard hull traces | Core | Finite endpoints, validated skip identity, owned results. A missing hit handle does not imply no collision. |
| Model and sound precaching | Experimental; Linux observations recorded | Loading-only callback, static validated names, distinct map-scoped indices. Linux runs cover registration, map changes and stale indices on Valve HLDS with classic Metamod, and ReHLDS with classic Metamod or Metamod-R. Win32 remains unverified. Native fatal load/capacity errors cannot be converted to Rust errors. [Contracts and results](resource-precaching.md). |
| Admission, keyvalue, and lag-compensation hooks | Core API; host validation required | Explicit hook opt-ins, owned observations, typed synchronous decisions, and validated `ClientRejectReason` values. Native fixture coverage does not establish live behavior on every host. |
| User-message payload construction | Core | Rust-only byte encoding and size validation, including the 192-byte payload budget. No implied schema correctness. |
| Message registration and recipient lookup | Core | Active-map observations; registration and session validity can expire across native calls. |
| Native message sending | Experimental, explicit unsafe opt-in | Reliable unicast only. Caller establishes idle entry, exclusive native ownership, recipient lifetime/readiness, and client protocol correctness. Byte-wise writers must be acceptable to native hooks. |
| Stock Ricochet fake-client adapter | Experimental, separate adapter | Unsafe deployment opt-in, game-specific setup, tracked identity, queued removal and later confirmation. Dropping a handle does not remove a client. |
| Generated `metamod-sys` bindings | Raw unsafe access | Pinned-header ABI access, outside the supported safe wrapper contract. Each native use needs its own validity, phase, lifetime, and reentry analysis. |
| Other hooks, entity mutation, movement, networking, sound playback, events, and remaining resource services | Outside initial wrapper scope | See the [coverage inventory](api-coverage.md) for available raw declarations and the [roadmap](roadmap.md#broader-sdk-feature-milestones) for future wrappers. |

“Core” selects APIs for release review; it does not waive host validation or
freeze their current signatures. Experimental APIs may change as their host
contracts are investigated. Keep the SDK game-agnostic; Ricochet setup and
message schemas belong in adapters or game-specific examples.

The [development priorities](roadmap.md#development-priorities) put generic
engine services, typed hooks, and validation first. Further Ricochet adapter
features and publication are deferred. Adapter validation is a release
requirement only if that adapter is included in the supported scope.

## Build targets

| Target | Proposed role | Current limits |
| --- | --- | --- |
| `i686-unknown-linux-gnu` | Primary native release target | Recorded ABI/loader and Linux live results exist. Minimum libc baseline and clean release-candidate validation remain open. Ubuntu 26.04 evidence does not establish an older libc baseline. |
| `i686-pc-windows-msvc` | Build/ABI target; live support pending | Windows x86 checks are configured in CI. Win32 admission, fake-client lifecycle, and message delivery remain unverified. Require results for the selected candidate before making release claims. |
| Other architectures and operating systems | Outside initial support scope | Host-target Rust tests do not establish a native GoldSrc deployment target. |

The documented build prerequisite is Rust 1.88 or newer, Clang/libclang, and
32-bit target C/C++ headers and linker support. Windows additionally needs the
Visual Studio C++ tools and Windows SDK. Minimum-version and distribution
checks remain release gates; see [build instructions](sdk-reference.md#build-and-install-hello-world).

## Host evidence

These tables describe recorded artifacts, not every version sharing a product
name. All named Linux configurations use i686 servers and stock Ricochet
unless stated otherwise.

| Label | Meaning |
| --- | --- |
| Recorded | The linked run passed its stated checks on identified artifacts. This does not establish a clean release-candidate build. |
| Development observations | Feature checks passed with development artifacts; the linked evidence explains unresolved provenance or certification requirements. |
| Unverified | No completed evidence for this feature/host combination. This does not mean failure. |
| Experimental | Support policy, independent of whether a particular test passed. |

### Compatibility and player lifecycle

| Engine and manager | Core v2 compatibility | Client admission | Ricochet fake-client lifecycle |
| --- | --- | --- | --- |
| Valve HLDS 8684 + classic Metamod 1.21.1-am | Recorded [C] | Recorded [A] | Recorded [F] |
| ReHLDS 3.15.0.896-dev + classic Metamod 1.21.1-am | Recorded [C] | Unverified | Recorded [F] |
| ReHLDS 3.15.0.896-dev + Metamod-R 1.3.0.149 | Recorded [C] | Recorded [A] | Unverified |
| Valve HLDS + Metamod-R | Unverified; experimental host combination | Unverified | Unverified |
| Windows x86 host combinations | Unverified live | Unverified | Unverified |
| Other GameDLLs or host versions | Unverified | Unverified | Unverified |

### Engine services

| Engine and manager | Server-state snapshots | Entity relationships | Model/sound precaching | TextMsg / ScreenFade delivery |
| --- | --- | --- | --- | --- |
| Valve HLDS 8684 + classic Metamod 1.21.1-am | Development observations [S] | Recorded [R] | Recorded, experimental [P] | Recorded, experimental sender [M] |
| ReHLDS 3.15.0.896-dev + classic Metamod 1.21.1-am | Development observations [S] | Recorded [R] | Recorded, experimental [P] | Recorded, experimental sender [M] |
| ReHLDS 3.15.0.896-dev + Metamod-R 1.3.0.149 | Development observations [S] | Recorded [R] | Recorded, experimental [P] | Recorded, experimental sender [M] |
| Valve HLDS + Metamod-R | Unverified | Unverified | Unverified | Unverified |
| Windows x86 host combinations | Unverified | Unverified | Unverified | Unverified |
| Other GameDLLs or host versions | Unverified | Unverified | Unverified | Unverified |

### Evidence and qualifications

| Key | Source | Coverage | Qualifications |
| --- | --- | --- | --- |
| C | [Core v2 certificate](certifications/ricochet-ubuntu-core-v2/20261122T115759Z-6890/certificate.md) | Six Ubuntu 26.04 configurations: the three named Linux combinations with stock Ricochet and the identified `ricochet-updated` binary. | The certificate names Valve HLDS as 1.1.2.1 and ReHLDS as 3.15.0.896; feature tables use the investigated runtime banners. Artifact hashes, SDK revision and itinerary define its scope. Other feature columns do not extend to `ricochet-updated` on the strength of core v2. |
| A | [Admission results](runtime-audit.md#live-ricochet-client-admission-results) | Deferral, rejection and reconnect with a real client on two hosts. | Applies only to the recorded hosts and artifacts. |
| F | [Fake-client results](fake-client-lifecycle.md) | Setup, queued teardown, reuse, map changes and the stated reentry fixtures on two hosts. | The separate Ricochet adapter remains experimental and requires unsafe deployment opt-in. |
| S | [Server-state observations](runtime-audit.md#linux-server-state-observations) | `server.state.v1` passed on four map visits per host; component identity checks passed and all servers quit with exit zero. | The `server-state v1` evaluator returned **UNVERIFIED** because plugin source was dirty and the copied runner lacked clean checkout identity. Clean-candidate reproduction is required. |
| R | [Relationship results](entity-relationships.md) | Live links, target removal/reuse and map changes on all three named Linux configurations. | Probe and native fixture were development builds. These observations do not extend core v2; Win32 remains unverified. |
| P | [Precaching results](resource-precaching.md#live-linux-results) | Model/sound registration, duplicate requests, map changes and stale indices on all three named Linux configurations. | The probe was a development build; identical binaries were used across hosts. Runs were uninstrumented and server-only. Client downloads, rendering and playback remain unverified. |
| M | [Message results](user-message-reentry.md#live-scoped-send-delivery-on-linux) | Client console delivery, exact demo payload bytes and normal shutdown. | No message-hook plugins were loaded. Sender and recorder were development builds; their hashes do not establish a clean build of current HEAD. |

Server-state snapshots were added after the historical core v2 run. The
optional `server-state v1` profile requires the new probe on every map visit,
in addition to core v2 evidence. Its observations remain separate from that
certificate; Windows and other host/GameDLL combinations are unverified.

The three named stock Ricochet combinations are the proposed primary Linux
release hosts. Each needs a clean candidate run for every core feature claimed
on that host. Keep `ricochet-updated` as historical compatibility coverage until
it is explicitly selected and retested for release. Other games remain usable
for experimentation, without inheriting Ricochet's validation.

## Contracts and release claims

All native APIs depend on the [host memory and registration contracts](sdk-reference.md#native-memory-and-registration-contracts).
Native calls can synchronously reenter. Hold no registry borrow across them,
never dispatch nested user callbacks while plugin state is mutably borrowed,
and revalidate identities after calls. Pointer shape checks cannot prove
foreign allocation validity.

Plugins advertise `PT_NEVER` and refuse detach. Stop the server before removing
or replacing a plugin. Shutdown may be omitted by the host. Registration storage
and callbacks must stay resident. A send guard never calls native `MessageEnd`
on drop; interruption can mean ownership has already been lost.

Compatibility, Rust soundness assessment, and native memory findings remain
separate. This matrix changes no core/player/extended v2 policy or historical
certificate. Publish a release support claim only with candidate build identity,
component hashes, required feature results, and remaining host obligations.
Do not promote a blank cell because another feature or neighboring host passed.

## SDK skill

The [Metamod SDK skill](../skills/metamod-sdk/SKILL.md) routes plugin tasks to
this matrix, matching Rustdoc, and examples. Use it with the SDK revision the
plugin depends on; its instructions do not extend the evidence in this matrix.
