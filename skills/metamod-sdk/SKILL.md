---
name: metamod-sdk
description: Build, update, or review GoldSrc Metamod plugins using metamod-sdk-rs. Select APIs and examples for the user's SDK revision and host, preserving lifecycle, reentry, and unsafe host contracts. Use for SDK consumers, not unrelated Rust or Source-engine plugins.
---

# Metamod SDK plugins

Help the user implement their plugin with the SDK APIs available at their
revision. Prefer the core wrappers. Keep experimental features and missing host
evidence visible without blocking ordinary Rust-only work.

## Resolve the SDK and host

Inspect the consumer's `Cargo.toml`, `Cargo.lock`, enabled features, and existing
plugin code. Locate the matching SDK checkout or dependency source. This skill
ships at `skills/metamod-sdk` in the SDK repository; the links below resolve
from there. If installed elsewhere, resolve those paths against the matching SDK
root rather than the skill's installation directory. Do not assume the consumer's
working directory is the SDK repository or substitute current upstream docs for
an older dependency revision. If sources are unavailable, identify that gap
before relying on an unconfirmed signature.

Read [API and host support](../../docs/support-matrix.md) and the relevant API's
Rustdoc in the matching source before choosing an implementation. Establish the
native target, engine/version, Metamod implementation/version, GameDLL/version,
and requested feature from project context. Ask only for missing details that
change the implementation or support claim. If the host is unknown, proceed
with host-independent work and label native behavior unverified.

The matrix separates proposed release scope from historical evidence. “Core”
is not a stable API promise. Check the specific feature/host cell: do not extend
core v2 results to admission, fake clients, messaging, another game, or current
binaries. Windows build/ABI checks do not establish Windows live support.
Keep compatibility, Rust soundness assessment, and native memory findings
separate. Read the linked investigation when the task depends on its result.

## Choose an existing pattern

Use the relevant row; there is no need to read every example.

| Task | Source and example |
| --- | --- |
| Start a plugin, metadata, manual registration | [crate Rustdoc and `Plugin`](../../metamod-sdk-rs/src/lib.rs), [hello-world](../../examples/hello-world/src/lib.rs) |
| Commands, CVars, validated names, player information | [engine Rustdoc](../../metamod-sdk-rs/src/engine.rs), [engine-demo](../../examples/engine-demo/src/lib.rs) |
| Retain a player selection, console/log output | [runtime Rustdoc](../../metamod-sdk-rs/src/runtime.rs) |
| Entity discovery, owned snapshots, line/hull traces | [trace Rustdoc](../../metamod-sdk-rs/src/trace.rs), engine-demo command handlers |
| Admission, keyvalues, synchronous pre/post decisions | [hook Rustdoc](../../metamod-sdk-rs/src/hooks.rs), `Plugin` opt-ins, engine-demo |
| Build or validate user-message bytes | [message Rustdoc](../../metamod-sdk-rs/src/message.rs) |
| Native user-message delivery | Message Rustdoc's unsafe contract, [ownership and delivery investigation](../../docs/user-message-reentry.md), [isolated Ricochet fixture](../../examples/engine-demo/examples/message_delivery.rs) |
| Fake-client setup and queued removal | [fake-client Rustdoc](../../metamod-sdk-rs/src/fake_client.rs), [Ricochet adapter](../../adapters/ricochet/src/lib.rs), [lifecycle investigation](../../docs/fake-client-lifecycle.md) |
| Build, install, or troubleshoot host prerequisites | [SDK reference](../../docs/sdk-reference.md) |
| A requested operation has no wrapper | [coverage inventory](../../docs/api-coverage.md); explain the gap before proposing raw FFI |

Confirm names and signatures in source, including required opt-in constants.
Use `metamod-sdk-rs` as the dependency and `metamod_sdk_rs` as the import.
Plugins are `cdylib` libraries. Default features provide `#[metamod_plugin]`;
when defaults are disabled, use `Metadata` and `export_plugin!`. Preserve the
consumer's chosen registration path.

## Preserve callback and identity rules

- Register commands and CVars once from an attached callback such as
  `start_frame`, with an attempt flag set before registration. `initialize`
  can decline attachment and cannot register them. Storage is bounded and
  permanent; a failed attempt can still consume a reservation.
- Keep plugin state on the server thread. Native calls can synchronously
  reenter. Never hold registry borrows across them or dispatch nested user
  callbacks while plugin state is mutably borrowed. Owned notifications can
  be deferred; nested frames are skipped, and nested synchronous decisions
  are ignored. Do not turn a synchronous decision into a queued result.
- Store full `PlayerId` or `EntityHandle` values, not just indices. Player slots
  are reusable; native user IDs are distinct from SDK sessions. Revalidate
  after native calls. Owned snapshots remain readable but do not pin entities
  or guarantee that their contents are still current.
- Use bytes or handle UTF-8 conversion failure for native strings. Respect
  documented byte limits and truncation. Null/alignment/length checks cannot
  establish arbitrary foreign pointer validity.
- Keep startup-only loading, `PT_NEVER`, detach refusal, and resident native
  registrations. Stop the server before replacing/removing the library.
  Shutdown delivery is optional; cleanup cannot rely on it always arriving.

Use each API's full contract for details; these rules do not replace its
`# Safety` section. If touching unsafe SDK internals, retain narrowly scoped
unsafe operations and `deny(unsafe_op_in_unsafe_fn)`.

## Handle experimental native operations

Do not add an unsafe opt-in merely to satisfy the compiler. First identify
which deployment facts establish its contract. If those facts are missing,
complete payload construction or other independent work, explain the missing
contract, and leave native sending/setup unimplemented rather than fabricate a
safe wrapper. The user may explicitly request a raw integration, but that still
requires an accurate unsafe boundary and does not create validation evidence.

For messaging, read `Context::message_sender` before generating a call. Its
contract covers idle entry, ownership/noninterference through begin/write/end,
recipient allocation and map lifetime, client readiness, and protocol schema.
Byte-wise writes must be acceptable to message hooks. SDK guards reject nested
SDK sends only; portable Metamod cannot exclude arbitrary native senders.
Never call `MessageEnd` from drop/unwind cleanup after interruption. Success
means the native sequence returned, not acknowledged client delivery. The
Ricochet TextMsg/ScreenFade fixture is game-specific test evidence, not a
universal sender recipe.

For fake clients, keep GameDLL setup in a separate adapter. The stock Ricochet
adapter does not establish another game's lifecycle. Creation failure may leave
an allocation the SDK cannot roll back. Queued removal is not confirmed removal;
observe `Removed` or `Invalidated` later. Dropping a handle neither kicks the
client nor cancels a queued command. Check the adapter's user-ID reuse and
queued-command lifetime requirements.

## Verify the result

Use the consumer's target and feature configuration. For SDK changes, focused
checks from the SDK root include:

```sh
cargo fmt --all -- --check
cargo check --locked --workspace --all-targets
cargo check --locked -p metamod-sdk-rs --no-default-features
cargo test --locked -p metamod-sdk-rs --doc
cargo test --locked -p metamod-sdk-rs --doc --no-default-features
```

Run relevant native fixtures for ABI or FFI changes using the
[verification guide](../../docs/sdk-reference.md#verify). For Rustdoc changes,
build with `RUSTDOCFLAGS='-D warnings'` in both feature configurations.
Doctests that define a callback/helper need not execute it; check its lifecycle
placement against the runtime, especially registration during attach.

Distinguish compilation, mock/fixture checks, and live observations in the
result. State which host/feature remains unverified and which checks were
actually run. A skill invocation does not authorize SSH, deployment, client
control, or overwriting a server installation; live testing needs the user's
applicable authorization and isolated installations.
