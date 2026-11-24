# GoldSrc user-message reentry

For plugin use, start with the [scoped sender](#scoped-sender). See
[delivery results](#live-scoped-send-delivery-on-linux) and
[reproduction steps](#reproduce-delivery) for the client tests.
The [ownership investigation](#exclusive-ownership-investigation) explains the unsafe host contract.

## Live Linux i686 observations

The disposable GameDLL opened a message, wrote one byte through Metamod,
and called `MessageEnd`. A separate Metamod plugin injected one operation
from its write pre-hook, or a complete second message from its end
post-hook. It used a one-shot guard. No client was connected. Each case
ran in a fresh loopback-only server process with classic Metamod 1.21.1-am.

| Probe | Valve HLDS build 8684 | ReHLDS 3.15.0.896-dev |
| --- | --- | --- |
| Second begin from the write pre-hook | Fatal diagnostic for an unfinished message, exit 255 | Same fatal diagnostic, then SIGSEGV, exit 139 |
| Extra byte from the write pre-hook | Nested write and outer end returned, clean exit | Nested write and outer end returned, clean exit |
| Write and end from the write pre-hook | Nested end returned. The outer write failed because no message was active, exit 255 | Same fatal diagnostic, then SIGSEGV, exit 139 |
| Complete second message from the end post-hook | Both message sequences returned, clean exit | Both message sequences returned, clean exit |

A successful return in the extra-byte case does not verify payload
integrity. The probe ran without recipients and did not capture packet
bytes. It registered the fixture messages on the first frame. The test
does not show whether fixed-size checking or delivery occurred. ReHLDS
source shows that writes append to the shared current buffer and that
registered message sizes may be checked later in `MessageEnd`. [ReHLDS message implementation](https://github.com/rehlds/ReHLDS/blob/1963001/rehlds/engine/pr_cmds.cpp#L2108-L2385)

## SDK boundary

- Keep payload construction, registered ID and size lookup, and recipient
  validation available as separate operations. They leave native message
  state untouched.
- A portable safe send needs exclusive ownership of the engine's
  in-progress message from begin through end. A Rust guard can exclude
  nested SDK sends. It cannot detect a message opened by the GameDLL,
  serialize unrelated native plugins, or prevent a hook from writing or
  ending the same message. GameDLL calls pass through Metamod wrappers
  on the tested stacks. [Classic Metamod wrappers](../upstream/metamod-hl1/metamod/engine_api.cpp)
- The successful post-end case establishes that the guard's second
  sequence returned after the original engine end in that fixture.
  A post-hook on its own cannot prove that another plugin did not
  supercede the original end or open another message. The public HLSDK
  messaging table has no portable ownership query or abort operation.
- Any future send path also needs to validate the registered identity and
  size, the message-specific payload schema, the current recipient, and
  delivery phase. A valid byte length alone says nothing about what the
  receiving client will parse.
- Require an explicit unsafe host opt-in for native sending. The scoped
  sender described below relies on the caller establishing that integration
  contract. Do not use native `MessageEnd` as automatic unwind cleanup
  when ownership may already have changed.

## Exclusive ownership investigation

The portable HLSDK/Metamod interfaces provide no acquisition token, atomic
send, current-message ownership query, or abort operation. Server-thread
confinement prevents concurrent Rust access, but native calls on that same
thread can reenter. The existing live results demonstrate that interference.

Classic Metamod copies the original engine table for plugins and replaces
registration-related entries. Message entries retain the engine callbacks.
It passes that table to plugin `GiveFnptrsToDll`. GameDLL calls instead go
through the pre/engine/post wrappers. Consequently, even observing every
GameDLL message hook cannot account for all plugin sends. This is a normal
interface path. [Plugin engine table](../upstream/metamod-hl1/metamod/metamod.cpp) · [Plugin handoff](../upstream/metamod-hl1/metamod/mplugin.cpp)

| Candidate mechanism | Ownership conclusion |
| --- | --- |
| SDK flag or mutex | Can reject nested SDK sends. Cannot identify a preexisting native message or control other senders. Holding a blocking mutex across reentry can deadlock. |
| Track begin/end with Metamod pre/post hooks | Plugin engine-table calls bypass these hooks. Superceded calls also prevent a hook observation from proving an engine state transition. |
| Supercede foreign writes during an SDK send | Cannot intercept direct plugin engine calls. Suppressing only parts of another sender's sequence also changes that sender's semantics. |
| Send from StartFrame or defer until a later frame | Reduces exposure to message-hook reentry, but callback timing alone cannot prove the engine was idle on entry. |
| Use the plugin engine table directly | Avoids classic Metamod's GameDLL hook dispatch during the sequence. This can be useful under an audited host contract, but provides no ownership query and does not exclude engine extensions. |
| Save and restore native message globals | Outside the portable ABI. Restoring bytes cannot undo delivery, native callbacks, or lifecycle changes. |
| ReHLDS message manager | Supports message hooks and blocking, but exposes no public exclusive-send transaction. Requires a separate versioned integration audit. |

### ReHLDS-specific findings

At ReHLDS revision `196300131858d1e087267fb2fb60cb935f8f8873`,
the engine keeps shared message globals. Its message manager intercepts
begin/write/end and can invoke a registered hook chain at end. The public
`IMessageManager` v2.0 interface exposes version queries, per-message
blocking, and hook registration/removal. It does not expose a method to
submit an independently owned message, acquire exclusive ownership, or
query the current transaction. The internal message stack is implementation
state rather than a portable sender API. These facts provide no automatic ReHLDS-specific ownership guarantee.
[ReHLDS message-manager interface](https://github.com/rehlds/ReHLDS/blob/196300131858d1e087267fb2fb60cb935f8f8873/rehlds/public/rehlds/IMessageManager.h) ·
[Message-manager implementation](https://github.com/rehlds/ReHLDS/blob/196300131858d1e087267fb2fb60cb935f8f8873/rehlds/rehlds/rehlds_messagemngr_impl.cpp)

Valve HLDS's internal implementation is closed source. Its conclusion here
rests on the public ABI, classic Metamod source, and the named live probes.
No assumption about a private Valve lock or buffer layout is required.

### Live observer-bypass regression

On November 24, 2026, the strengthened `message-nested-write` probe ran on
fresh copies of the saved installations. It counts every entry into its
write pre-hook. Inside the GameDLL's observed byte write, it calls the
plugin engine-table byte writer and verifies that the hook-entry count
does not change. The one-shot reentry guard cannot conceal a nested entry
because counting precedes the guard.

| Linux i686 host with classic Metamod 1.21.1-am | Direct plugin write bypassed observer | Outer end returned | Exit |
| --- | --- | --- | --- |
| Valve HLDS build 8684 | PASS | Yes | 0 |
| ReHLDS 3.15.0.896-dev | PASS | Yes | 0 |

The transcript marker is
`MESSAGE_HOOK PASS plugin write bypassed observer`. The runner requires
that marker, outer completion, and exit zero. No clients were connected.
This verifies an observation gap, with no claim about delivered payloads.

The test-only plugin SHA-256 was
`09b66063b3d8d59d29a0a6f4ce6cde9e853a34b53fa5999c38f97d80e1597cd8`.
The supplied GameDLL SHA-256 was
`dd61d1a3733df5745b88d623f80e3ad4c094d1addeb8b8baf0e7cf477fd37620`.
These builds differ from the earlier probes above. They were development
artifacts, without a clean-commit build attestation.

| Artifact | SHA-256 |
| --- | --- |
| Shared launcher | `f8a46f49c43ceb4a744bc4a415f4360f7fb93ac112a76616f84dd52c26168e18` |
| Valve engine module | `52e7ed23455eeb49a1561114fcde230b738cf94b40e2d77953555fb32ed07ba8` |
| ReHLDS engine module | `5bb5d862b7e65ced8c9cc5a2159d52d5522e3eb784ba0eb88e6624a410566e99` |
| Classic Metamod | `6956fb8be550189c827f14b919d998551c81b34e75becbb705c0b085d5390a20` |
| Valve transcript | `b2c7fc2483b7758a844fb4e4c43e2d24671176d262d7d34c070cfb262490d9c5` |
| ReHLDS transcript | `17ad0d26cd28868e0367a0f44fe753d53bc7117fb19d00042c1ddd39555bd6a0` |

Logs and exit records remain on the audit host under
`~/goldsrc-message-ownership.db2SLi/nested-write/`. This is an isolated
fixture observation, separate from core v2 certification. The previous
fatal reentry cases were not repeated in this run.

### What the SDK can expose

Owned payload construction, registered-message lookup and size validation,
and session-aware recipient validation remain available without a host opt-in.
An observational hook can copy bounded native inputs into owned events,
but any future implementation must specify when observations expire.

An unconditional safe begin/write/end builder is not justified by these
interfaces. In particular, exposing a guard whose destructor calls
`MessageEnd` would assume ownership after reentry or unwinding.

The scoped sender requires its deployment contract through an explicit unsafe
opt-in. Establish all of the following before using it:

- Native message state is idle at entry, with a documented phase or host
  mechanism establishing that fact for every supported call.
- The chosen engine callbacks cannot reenter an uncontrolled sender,
  retire the map, or invalidate the recipient during the sequence.
  Account for ReHLDS extensions and message-manager hooks explicitly.
- Validation and allocation finish before begin. The transaction invokes
  no plugin closures, logging, or fallible formatting.
- Registration, payload schema, size, recipient session, and delivery phase
  are checked at the actual send boundary.
- The host supplies synchronous buffer and edict lifetimes, respects the
  native ABI, and has documented failure behavior for the full sequence.

The SDK can enforce its own thread, nested-send, and validation rules.
Deployment-specific idle-state and native non-reentry guarantees require
external evidence. A client delivery test would validate selected messages
on that deployment, but would not establish exclusive ownership among all
native senders. Win32 and adversarial Metamod-R ownership behavior need
separate checks. The scoped delivery check below covers a controlled
Metamod-R deployment, without claiming universal exclusivity.

## Scoped sender

`unsafe { ctx.message_sender() }` returns a callback-scoped, server-thread
sender. Its safe `send_to(name, &payload, player_id)` method sends reliable
unicast (`MSG_ONE`). The unsafe constructor requires the operator/plugin
author to establish idle entry, exclusive native ownership, synchronous
lifetimes, a supported delivery phase, and valid client protocol payloads
for every use of that sender. This is an external host obligation.

The method checks the callback's active map, required engine callbacks,
fresh registration and declared size, and a freshly resolved `PlayerId`.
A thread-local guard rejects nested sends from any SDK sender, including
during validation. The payload is fully assembled before begin. It sends
each encoded byte through the native byte writer, preserving little-endian
fields and string terminators without native format interpretation. Native
message-manager hooks see byte parameters, so this backend requires those
hooks to accept byte-wise encoding or be absent. It does not preserve the
original typed writer boundaries for third-party hook inspection.

Map or shutdown interruption stops further writes/end and returns
`MessageSendError::Interrupted`. Recipient invalidation also interrupts
the sequence. This avoids continuing after an observed invalidation but
cannot undo bytes or restore ownership after a host-contract violation.
Dropping the sender or internal guard never calls native end. A successful
return indicates that the native sequence returned, without a client
acknowledgement.

The [delivery fixture](../examples/engine-demo/examples/message_delivery.rs)
demonstrates the opt-in for isolated stock Ricochet with classic Metamod
or the reviewed Metamod-R deployment.
It sends the known `TextMsg` schema and an inert fixed-size `ScreenFade`.
It is a deployment-specific test plugin. The SDK itself remains
game-agnostic. Arbitrary byte payloads require their own client schema review.

Focused Rust/Miri mocks cover encoded bytes, unavailable callbacks, stale
recipients, size failure before begin,
nested-send rejection, and stopping without native end after map retirement
or shutdown. The interruption cases intentionally violate the opt-in using
valid mock allocations, with no arbitrary invalid pointer dereferences.

## Live scoped-send delivery on Linux

On November 24, 2026, the SDK delivery example ran on separate copies of
the stock Ricochet installations with classic Metamod 1.21.1-am. The
operator's Steam Ricochet client connected over the test network.
The server console invoked the sender after client signon and after
recording began. No native message interception plugins were loaded.
A separate test-only recorder plugin queued fixed record/stop client
commands from the server console; it installs no message hooks.

| Runtime | Client console text | Exact variable and fixed payload pair in client demo | Server quit |
| --- | --- | --- | --- |
| Valve HLDS build 8684, Linux i686 | PASS | PASS | Exit 0 |
| ReHLDS 3.15.0.896-dev, Linux i686 | PASS | PASS | Exit 0 |

Both demos declare `TextMsg` as native ID 77, variable size, and
`ScreenFade` as native ID 95, fixed size 10. The client console printed
`SDK_MESSAGE_INTEGRITY_254_255_16909060`. The matching TextMsg wire payload
was 45 bytes, including HUD_PRINTCONSOLE, the text/newline, and five NULs.
The adjacent fixed-size ScreenFade payload was exactly
`00 00 00 00 00 00 fe ff 11 00`. Zero alpha and duration made the effect
visually inert. These cover selected schemas and reliable single-player
delivery, including high-bit bytes. They do not verify other message
schemas, destinations, or concurrent/reentrant native senders.

The bounded [demo checker](../verify/user-message/check-demo.py) verifies
the GoldSrc demo header, unique matching registration IDs/sizes, and one
exact contiguous encoded payload pair. It uses byte matching rather than
a complete demo protocol decoder. Console output independently confirms
that the client's TextMsg parser processed the text.

The stock GameDLL hash was
`57ea9f913b543d9fdfedcab43edfef8fcf5754fc9fada2bee6cac620eb4ce3e4`.
Launcher, engine module, and classic Metamod hashes match the ownership
regression table above. Additional artifacts follow.

| Artifact | SHA-256 |
| --- | --- |
| SDK delivery example | `bbf4cac94208e4ec0463811aa7955ece90f55dbb55be5fd07b599728c667f390` |
| Recording helper | `7993c9f4a8c9b8c2299de67c7703e03b100b09804bd86f12c30bdb2d29618389` |
| Valve client demo | `6e3cb1163589bd52ed97a204bd220e5ac2446aa05b42a51849fa4583d78ca13f` |
| ReHLDS client demo | `ac6ef9e857e97c3cba30ab8c1fb05960c8fa6f9118a8d72f09c4c03cd2497dd6` |
| Valve server log | `4c71f1c5060eec88f49fba46ac1002049ccbb421c2b1ab6293e3fec24ff649f7` |
| ReHLDS server log | `728424c86897f9db81074116cfd10fa0113d3710ca783cab08f573698bff512e` |

These are development builds from an uncommitted working tree, with no
clean-commit provenance claim. Server logs, demos, and exit records are
preserved on the audit host under `~/ricochet-message-delivery.pSU34O/`.
They are separate from compatibility certification and were not run under
live-server sanitizers. The client emitted existing missing-sprite warnings
and an unknown `fullupdate` command when demo recording started; the text
and exact payload checks still passed. Win32 and other clients remain
unverified. A subsequent Metamod-R run follows.

### ReHLDS with Metamod-R

A separate isolated stock Ricochet run on November 24, 2026 used
ReHLDS 3.15.0.896-dev with Metamod-R 1.3.0.149, revision `603a257`,
on Linux i686. It reused the exact sender and recording-helper binaries
listed above. Metamod-R reported both fixtures running, with no additional
plugins loaded.

The matching Metamod-R implementation copies the original engine table,
replaces selected registration callbacks, and passes the resulting table
to plugin `GiveFnptrsToDll`. Its message entries therefore take the direct
engine path used by this fixture. No message-manager hooks were registered
by the test plugins. The send was invoked from the server console after
client signon. This review supports the explicit opt-in for this controlled
test. It provides no global ownership mechanism for arbitrary plugins.
[Metamod-R engine table](https://github.com/rehlds/Metamod-R/blob/603a2574e937171aa5e737bd5e7b4a07f74e88ee/metamod/src/metamod.cpp) ·
[Plugin handoff](https://github.com/rehlds/Metamod-R/blob/603a2574e937171aa5e737bd5e7b4a07f74e88ee/metamod/src/mplugin.cpp)

| Check | Result |
| --- | --- |
| Expected ReHLDS/Metamod-R identities and both fixtures running | Observed |
| TextMsg text displayed in the client console | PASS |
| Exact adjacent 45-byte TextMsg and 10-byte ScreenFade payloads in client demo | PASS |
| TextMsg ID 77 with variable size, ScreenFade ID 95 with fixed size 10 | PASS |
| Normal server quit | Exit 0 |

The ReHLDS engine and stock Ricochet hashes match the preceding run.
The Metamod-R and evidence hashes follow.

| Artifact | SHA-256 |
| --- | --- |
| Metamod-R | `8bed09c362f6d9019ce7d88b9a19d1ad08eca26bf46c0ed32604fbea2d7ba7a9` |
| Client demo | `70d1bd8af91ae87d8fd85732026e9b24a88f9b6ec3cd89bf77d62c770a0b9ecf` |
| Client console log | `b01934e18f5723fc0de94cd3a878c2af6bf3abbcf4fb1eea2cfab4a0cc0509d1` |
| Server log | `db2d26ef4e4894079ba47e9fd4ed3f032fb4d04dced6ca098ec3684a8e6f3fb8` |

The server, launch script, console FIFO, logs, exit record, and copied
client evidence remain under
`~/ricochet-message-delivery-mmr.SsLi8E/` on the audit host.
Original installations and earlier evidence remain intact. This run was
uninstrumented and separate from core v2 certification. The same development
binary provenance limitations apply. Existing missing-sprite warnings and
the recording-time unknown `fullupdate` command also occurred, while the
exact delivery checks passed.

### Reproduce delivery

Build on a development machine with the pinned submodules and x86 tools.

```bash
cargo build --locked --release --target i686-unknown-linux-gnu \
  -p engine-demo --example message_delivery
bash verify/fake-client/build-linux-i686.sh /tmp/message-recorder.so recorder
```

Transfer `target/i686-unknown-linux-gnu/release/examples/libmessage_delivery.so`
and the recording helper to a fresh isolated stock Ricochet installation.
Use classic Metamod or the reviewed ReHLDS/Metamod-R combination with two
client slots, configure only these two test plugins at startup, and
use the same Linux library path and Valve alias requirements as the
[runtime runner](runtime-audit.md#retesting-the-two-valve-hlds-configurations).
Use an available LAN port reachable by the consenting test client. The
loopback-only reentry runner below is for the disposable GameDLL experiments.

After the client is fully connected, issue `sdk_record_client` in the
server console. Wait for the client's recording confirmation, then issue
`sdk_message_probe`, confirm the text on the client, and issue
`sdk_stop_recording`. Copy the client's
`ricochet/sdk_message_delivery_20261124.dem` to a distinct filename before
testing the next server, since the recording name is fixed.

```bash
python3 verify/user-message/check-demo.py /path/to/copied-client.dem
```

Stop each isolated server with `quit`, preserving logs, exit status,
component hashes, and the actual source/build provenance of any rebuilt
fixtures. Do not deploy the unsafe opt-in based solely on a matching
engine version label. Review the complete host and client protocol contract.

## Reproduce

Build both artifacts on a Linux development machine with the pinned
headers and a 32-bit C++ toolchain.

```bash
bash verify/fake-client/build-linux-i686.sh /tmp/message-gamedll.so
bash verify/fake-client/build-linux-i686.sh /tmp/message-reentry.so message
```

Transfer both artifacts and the runner to the audit host. On a disposable
host, run each case from the saved snapshot directory into a fresh
output directory. The runner records the expected fatal cases as
successful probes only when the expected diagnostic and exit are present.
The saved server installations are left untouched.

```bash
for mode in post-end nested-begin nested-write nested-end; do
  bash verify/fake-client/run-linux.sh /path/to/saved/installs \\
    /path/to/message-reentry.so "/path/to/new-output/$mode" \\
    "message-$mode" /path/to/message-gamedll.so
done
```

Use an existing output parent with enough disk for the disposable copies.
The tested GameDLL SHA-256 was
`b0aafb432f38ae2f57035aeba0c44cc5de21f93d5e5105b4893799d9539b8481`.
The tested Metamod probe SHA-256 was
`4dcd62476c598055544d63a8aad31aa886e8049eb760d5e9251ba9420a53f368`.
Win32, other managers, and map transitions during an open message remain
unverified. Recipient delivery was subsequently checked by the separate
stock Ricochet scoped-send runs documented above.

## Verification commands for the scoped sender

```bash
cargo fmt --all -- --check
cargo check --locked --workspace --all-targets
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo check --locked -p metamod-sdk-rs --no-default-features
cargo +nightly miri test --locked -p metamod-sdk-rs message_send_preserves_bytes_and_rejects_nested_and_interrupted_sends
cargo +nightly miri test --locked -p metamod-sdk-rs send_guard_releases_after_rust_unwind
METAMOD_SANITIZE_LOADER=1 bash verify/linux-i686.sh
python3 -m unittest discover -s verify/audit
```

These checks passed locally for this increment, including the two Miri
scenarios and the Linux i686 C++ ASan/UBSan/LSan loader. The existing
Python suite passed 51 tests with one Windows-only skip. The sender
example built as Linux i686. The demo checker accepted both live captures
and rejected corrupted headers and test text. ShellCheck was unavailable.
A Win32 cross-check could not generate bindings because this Linux
environment has no Windows SDK `windows.h`; native Windows verification
remains for the existing Win32 CI job. The live server runs were
uninstrumented. This pass does not claim Rust ASan coverage of the new
sender based on the C++ loader run.
