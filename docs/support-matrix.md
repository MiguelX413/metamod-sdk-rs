# API and host support

This is the proposed first stable-release scope, based on the repository's
recorded evidence as of 2026-11-24. It is not a certification of the current
working tree or an API stability promise for the development version.
The [release roadmap](roadmap.md) lists the work required before release.

## API scope

| API family | Proposed release status | Boundary |
| --- | --- | --- |
| Plugin registration and runtime | Core | Attribute registration with default features; manual `Metadata` and `export_plugin!` with default features disabled. Startup loading only, server-thread state, deferred owned notifications, panic containment, and optional shutdown delivery. |
| Console, commands, and CVars | Core | Validated names and values, owned command tokens and CVar reads, bounded permanent registrations. Register once from an attached callback, not `initialize`. |
| Player identity and information | Core | Distinct slot, native user ID, and tracked session types; owned information snapshots. Tracking covers observed joins. Revalidate saved sessions. |
| Entity discovery and snapshots | Core | Map epochs and native serials, bounded index lookup, owned common fields and strings. No general entity mutation or GameDLL-private data access. |
| Line and standard hull traces | Core | Finite endpoints, validated skip identity, owned results. A missing hit handle does not imply no collision. |
| Admission, keyvalue, and lag-compensation hooks | Core API; host validation required | Explicit hook opt-ins, owned observations, typed synchronous decisions. Native fixture coverage does not establish live behavior on every host. |
| User-message payload construction | Core | Rust-only byte encoding and size validation, including the 192-byte payload budget. No implied schema correctness. |
| Message registration and recipient lookup | Core | Active-map observations; registration and session validity can expire across native calls. |
| Native message sending | Experimental, explicit unsafe opt-in | Reliable unicast only. Caller establishes idle entry, exclusive native ownership, recipient lifetime/readiness, and client protocol correctness. Byte-wise writers must be acceptable to native hooks. |
| Stock Ricochet fake-client adapter | Experimental, separate adapter | Unsafe deployment opt-in, game-specific setup, tracked identity, queued removal and later confirmation. Dropping a handle does not remove a client. |
| Generated `metamod-sys` bindings | Raw unsafe access | Pinned-header ABI access, outside the supported safe wrapper contract. Each native use needs its own validity, phase, lifetime, and reentry analysis. |
| Other hooks, entity mutation, movement, networking, sound, events, and resources | Outside initial wrapper scope | See the [coverage inventory](api-coverage.md) for available raw declarations and the [roadmap](roadmap.md#broader-sdk-feature-milestones) for future wrappers. |

“Core” selects APIs for release review; it does not waive host validation or
freeze their current signatures. Experimental APIs may change as their host
contracts are investigated. Keep the SDK game-agnostic; Ricochet setup and
message schemas belong in adapters or game-specific examples.

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

The table describes historical results, not all versions sharing a product
name. **Recorded** means the linked run passed its stated checks on identified
artifacts. **Unverified** means this repository has no completed evidence for
that feature/host cell. It does not mean failure. **Experimental** describes
support policy, independently of whether a particular test passed.

All rows below are Linux i686 with stock Ricochet unless stated otherwise.

| Engine and manager | Core v2 compatibility | Client admission | Ricochet fake-client lifecycle | TextMsg / ScreenFade delivery |
| --- | --- | --- | --- | --- |
| Valve HLDS 8684 + classic Metamod 1.21.1-am | Recorded [C] | Recorded [A] | Recorded [F] | Recorded, experimental sender [M] |
| ReHLDS 3.15.0.896-dev + classic Metamod 1.21.1-am | Recorded [C] | Unverified | Recorded [F] | Recorded, experimental sender [M] |
| ReHLDS 3.15.0.896-dev + Metamod-R 1.3.0.149 | Recorded [C] | Recorded [A] | Unverified | Recorded, experimental sender [M] |
| Valve HLDS + Metamod-R | Unverified; experimental host combination | Unverified | Unverified | Unverified |
| Windows x86 host combinations | Unverified live | Unverified | Unverified | Unverified |
| Other GameDLLs or host versions | Unverified | Unverified | Unverified | Unverified |

- **[C]** The [core v2 certificate](certifications/ricochet-ubuntu-core-v2/20261122T115759Z-6890/certificate.md)
  records six Ubuntu 26.04 configurations: the first three combinations with
  stock Ricochet and with the identified `ricochet-updated` binary. It names
  Valve HLDS as 1.1.2.1 and ReHLDS as 3.15.0.896. Artifact hashes, SDK revision,
  and itinerary in that certificate define its scope; the version labels above
  follow the feature investigations. No other column extends to
  `ricochet-updated` on the strength of core v2 alone.
- **[A]** [Admission results](runtime-audit.md#live-ricochet-client-admission-results)
  cover deferral, rejection, and reconnect with a real client on two hosts.
- **[F]** [Fake-client results](fake-client-lifecycle.md) cover setup, queued
  teardown, reuse, map changes, and the stated reentry fixtures on two hosts.
- **[M]** [Message results](user-message-reentry.md#live-scoped-send-delivery-on-linux)
  cover client console delivery, exact demo payload bytes, and normal shutdown.
  These controlled deployments had no message-hook plugins. The sender and
  recorder came from an uncommitted development tree; their hashes identify
  the artifacts, not a clean build of current HEAD.

The first three stock Ricochet rows are the proposed primary Linux release
hosts. Each needs a clean candidate run for every core feature claimed on that
row. Keep `ricochet-updated` as historical compatibility coverage until it is
explicitly selected and retested for release. Other games remain usable for
experimentation, without inheriting Ricochet's validation.

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
