# Metamod SDK for Rust

Write native Metamod server plugins in Rust for GoldSrc, the engine behind the
original Half-Life, Counter-Strike 1.6, Team Fortress Classic, and Ricochet.
Metamod is a plugin loader that lets plugins observe or intercept calls
between the server engine and a game's rules library (the game DLL).

Keep plugin state in Rust structs, handle game events through typed methods,
and use engine services through the SDK's wrappers.

## Features

- Handle server frames, map changes, and player events. Inspect or reject
  incoming connections.
- Print to the console, log through Metamod, register server commands, and
  work with console variables.
- Read server timing, capacities, map names, and game-mode values through
  owned snapshots.
- Precache models and sounds through a loading-only callback with static names.
- Find entities, inspect snapshots, and trace lines and hulls through the
  world. Player and entity handles help detect stale identities.

## Quick start

In a Rust library, add the SDK dependency and build the plugin as a native
shared library:

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
metamod-sdk-rs = { git = "https://github.com/MiguelX413/metamod-sdk-rs" }
```

Put this in `src/lib.rs` to print a greeting on the first server frame:

```rust
use metamod_sdk_rs::prelude::*;

#[derive(Default)]
struct MyPlugin {
    greeted: bool,
}

#[metamod_plugin(
    name = "My Plugin",
    version = "0.1.0",
    author = "Your Name",
    log_tag = "MYPLUGIN"
)]
impl Plugin for MyPlugin {
    fn start_frame(&mut self, ctx: &mut Context<'_>) {
        if !self.greeted {
            self.greeted = true;
            ctx.server().print(c"Hello from Rust!\n");
        }
    }
}
```

To try the bundled [`hello-world` plugin](examples/hello-world/src/lib.rs),
clone this repository with submodules. With
Rust 1.88 or newer, Clang/libclang, and a 32-bit C++ toolchain installed, run
these commands from the checkout on Linux:

```sh
rustup target add i686-unknown-linux-gnu
cargo build --locked --release --target i686-unknown-linux-gnu -p hello-world
```

Follow the [installation steps](docs/sdk-reference.md#install-the-plugin) to load
the resulting `libhello_world.so` on a server that already runs Metamod. The
reference also covers [Windows x86 builds](docs/sdk-reference.md#windows-x86).

The SDK keeps plugin state on the server thread and passes a scoped context
to each hook. The context exposes engine and Metamod services for that
callback. Low-level Rust bindings are generated at build time from pinned
upstream headers.

## Status and limitations

Build and ABI checks cover Linux i686 and Windows x86. The [live Ricochet core v2 results](docs/certifications/ricochet-ubuntu-core-v2/20261122T115759Z-6890/certificate.md)
cover six recorded Linux i686 configurations; live coverage varies by feature
and host.

Plugins load at server startup. Stop the server before removing or replacing
a plugin. The native host must honor the documented [FFI and lifetime contracts](docs/sdk-reference.md#architecture-and-safety).
Native user-message sending requires an explicit unsafe host opt-in; see
[messaging support and limits](docs/api-coverage.md#validated-user-messages).

## Documentation

- [API and host support](docs/support-matrix.md) for proposed release scope and recorded host evidence
- [SDK reference](docs/sdk-reference.md) for setup, API usage, and native safety contracts
- [Hello world](examples/hello-world/src/lib.rs), [engine demo](examples/engine-demo/src/lib.rs), and [player tracker](examples/player-tracker/src/lib.rs)
- [Release roadmap](docs/roadmap.md) for release gates, feature expansion, and validation
- [API coverage](docs/api-coverage.md) for the current interface inventory
- [Runtime auditing](docs/runtime-audit.md) and [memory-safety verification](docs/memory-safety-audit.md)
- [License](LICENSE) and [upstream notices](docs/sdk-reference.md#licensing-and-references)

## Use with a coding assistant

Give your assistant the [Metamod SDK skill](skills/metamod-sdk/SKILL.md) as
project instructions, an attached file, or a skill in its supported format.
It guides API selection, callback lifecycles, and host support checks without
requiring a particular model or editor. Installation and invocation depend on
the assistant; the Markdown body can also be used directly.

Keep the matching SDK sources available: the instructions read their support
matrix, Rustdoc, and examples rather than bundling a second copy of them.
