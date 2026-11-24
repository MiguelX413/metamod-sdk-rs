# Rustdoc audit: 2026-11-24

Audited on 2026-11-24 with `missing_docs` warnings enabled for
`metamod-sdk-rs`, with default features and with `--no-default-features`.
The initial audit reported **124 missing documentation comments** in both
configurations. This is a lint count, not a percentage of documented
API or an assessment of documentation quality. Private implementation details,
`#[doc(hidden)]` internals, generated raw bindings, and the separate macro and
adapter crates are outside this count. The macro's existing documentation is
available through its SDK re-export when default features are enabled.

| Source | Initial missing comments |
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

After the follow-up pass, both feature configurations built with zero
missing-docs warnings and the SDK enabled `deny(missing_docs)`. Rustdoc examples
cover registration, engine services, player identities, hooks, tracing, and
message validation. The crate homepage links to them.

These results measure lint coverage and compilation, not documentation quality,
complete unsafe contracts, or compatibility on an untested host.
