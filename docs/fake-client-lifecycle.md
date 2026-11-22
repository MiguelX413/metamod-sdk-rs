# Fake-client lifecycle investigation

The pinned GoldSrc engine table describes `pfnCreateFakeClient` as returning
an edict or null. It does not specify GameDLL initialization, callback order,
rollback, or client removal. The engine owns the client slot and edict.
[HLSDK engine table](../upstream/hlsdk/engine/eiface.h)

| Question | ReHLDS `1963001` source | Valve HLDS before live fixture verification |
| --- | --- | --- |
| Creation | The engine picks a slot with `active`, `spawned`, and `connected` clear. No free slot returns null before allocation. Success sets those flags, marks the client fake, assigns a user ID, and returns the slot edict. | The header establishes the null result. The original HPB bot implementation calls creation and then separately initializes the GameDLL player and calls `ClientConnect` and `ClientPutInServer`. That establishes how this caller used the interface, with the behavior of individual retail builds still unverified. |
| Callbacks during creation | The default implementation does not call `ClientConnect` or `ClientPutInServer`. It calls `SV_ExtractFromUserinfo` after claiming the slot, which invokes `ClientUserInfoChanged`. ReHLDS hook chains and Metamod wrappers provide additional reentry points. | Expect reentry conservatively. Callback order, including user-info notification, needs observation on a named build. |
| Partial failure | Once a slot has been claimed, the default implementation has no subsequent null-return rollback branch. A later GameDLL rejection or a result invalidated by reentry can leave the engine slot occupied. | Rollback after successful allocation and failed setup remains unverified. |
| Release | The engine's drop path clears all three slot-occupancy flags. On the non-crash path it calls `ClientDisconnect` for a spawned client with an edict **before** clearing them. The engine can also drop fake clients on level change. The drop path does not delete the edict. `pfnRemoveEntity` separately frees an edict and is not a client-slot release operation. | Historical callers use server kick commands. Whether a selected build delivers disconnect and releases or reuses the slot on each path needs testing. The portable ABI has no immediate drop-client callback. |

[ReHLDS creation](https://github.com/rehlds/ReHLDS/blob/1963001/rehlds/engine/pr_cmds.cpp#L1944-L2002) ·
[ReHLDS user-info dispatch](https://github.com/rehlds/ReHLDS/blob/1963001/rehlds/engine/sv_main.cpp#L5249-L5257) ·
[ReHLDS drop](https://github.com/rehlds/ReHLDS/blob/1963001/rehlds/engine/host.cpp#L424-L518) ·
[ReHLDS level change](https://github.com/rehlds/ReHLDS/blob/1963001/rehlds/engine/sv_main.cpp#L7603-L7614) ·
[ReHLDS edict removal](https://github.com/rehlds/ReHLDS/blob/1963001/rehlds/engine/pr_cmds.cpp#L905-L913) ·
[HPB bot creation](https://github.com/Bots-United/HPB-bot/blob/master/bot.cpp)

## SDK boundary

- Keep creation on the server thread in an active map. Assume the call
  can synchronously reenter the host. Hold no registry borrow across it,
  and check the map epoch and shutdown state before examining a returned
  edict. Then validate client capacity, slot identity, serial and fake-client
  state across any subsequent lookup. Report failure to obtain a validated
  identity separately from a null return. Failure after the native call may
  still have consumed a slot.
- Keep engine allocation and GameDLL admission distinct. A non-null edict
  does not establish a fully initialized GameDLL player. The current SDK
  establishes `PlayerId` through an observed `ClientPutInServer` hook.
  Direct calls through the original GameDLL table can bypass Metamod hook
  delivery. Avoid synthesizing an ordinary player-join event from allocation
  or promising that the existing admission hook runs for every fake client.
- Require an established GameDLL setup and teardown contract before exposing
  safe creation to arbitrary plugins. In the pinned Ricochet source,
  `ClientDisconnect` dereferences GameDLL private player data. ReHLDS marks
  an engine-created fake client spawned before that private data is guaranteed
  to exist. An immediate drop after allocation or failed setup can therefore
  reach GameDLL code with missing state. Slot allocation and rollback cannot
  be wrapped as a portable transaction. [Ricochet disconnect](../upstream/hlsdk/ricochet/dlls/client.cpp)
- Keep teardown explicit and host-scoped. A Rust destructor cannot assume a
  live server thread or map, and a queued server command cannot guarantee
  immediate completion. Confirm release independently of disconnect callback
  entry. Test for reuse even when the engine retains the same edict address.
  Once release or map retirement is observed, invalidate the old session
  and prevent it from targeting the next occupant.

## Live Linux i686 fixture results

Both isolated snapshots ran the fixture GameDLL behind classic Metamod
1.21.1-am on `rc_arena`. No SDK plugin or stock Ricochet GameDLL was
loaded. Both servers exited with status zero.

| Observation | Valve HLDS build 8684 | ReHLDS 3.15.0.896-dev |
| --- | --- | --- |
| `ClientUserInfoChanged` ran before each successful allocation returned | Observed | Observed |
| Allocation automatically called `ClientConnect` or `ClientPutInServer` | Neither observed | Neither observed |
| Third allocation with two occupied slots returned null | Observed | Observed |
| Explicit fixture setup rejection left both slots occupied | Observed | Observed |
| Console kick by user ID called `ClientDisconnect` on the unprepared slot | Observed | Observed |
| Next allocation reused slot 1, the same edict address, and the same serial | Observed | Observed |

The fixture deliberately called its own rejecting `ClientConnect` after
allocation. Its final `ClientPutInServer` call was also deliberate. Neither
event is evidence of automatic engine dispatch. Both hosts delivered more
user-info notifications after the probe and called disconnect on quit.
The fixture accepts those calls without GameDLL-private player state.

The same-slot result means an SDK fake-client handle needs a distinct
session identity even when the slot, edict address, serial and map all
match. A safe teardown cannot assume the ordinary GameDLL tolerates the
unprepared disconnect that this test fixture explicitly handles.

The tested fixture SHA-256 was
`a2f9d50f266335e81a10fe555c34d87276c5b3d68f4a1924ae81e2574b9b75f7`.
The fixture uses proprietary server assets only at runtime. The logs
belong to the isolated audit run. These results are separate from core v2
and from the prior live tests of real-client admission.

## Reproduce

Build on a Linux development machine with the pinned HLSDK and an i686 C++
toolchain. Transfer the resulting shared object and the run script to the
audit host. Pass the saved directory containing the `linux-hlds` and
`linux-rehlds` snapshots, the uploaded fixture, and a new output directory.

```bash
bash verify/fake-client/build-linux-i686.sh /tmp/fake_fixture.so
bash verify/fake-client/run-linux.sh /path/to/saved/installs \
  /path/to/fake_fixture.so /path/to/new-fixture-run
```

The script copies both snapshots, binds the test servers to loopback,
writes separate logs, and requires a fixture pass marker and clean exit
for each server. Use a test host and keep UDP port 27057 free. The fixture
does not implement gameplay or support real clients.

## Stock Ricochet setup and teardown

A separate [Metamod probe](../verify/fake-client/ricochet_probe.cpp) ran
with the stock Ricochet GameDLL on both Linux i686 snapshots above. It
called the original GameDLL's `ClientConnect` and `ClientPutInServer` in
that order after engine allocation, then queued a kick by the engine
user ID. Both hosts completed two setup and kick sequences and exited
cleanly.

| Observation | Valve HLDS build 8684 | ReHLDS 3.15.0.896-dev |
| --- | --- | --- |
| GameDLL accepted setup and created private player data | Yes | Yes |
| Engine set `FL_FAKECLIENT` at allocation | Yes | Yes |
| Ricochet cleared `FL_FAKECLIENT` during spawn | Yes | Yes |
| Kick delivered disconnect with private player data still present | Yes, twice | Yes, twice |
| Replacement reused the same slot, edict address and serial with a new user ID | Yes | Yes |

The first probe treated loss of the fake flag as a setup failure. The final
probe recorded the change and restored the flag for the client it had
allocated. This matches the pinned Ricochet spawn code, which assigns
`FL_CLIENT` to the flags field. The probe waits for the queued kick to
run. Assuming it would complete by the next frame caused a false failure
on both hosts. Direct calls to the original GameDLL table do not test
Metamod dispatch of the SDK's admission or join hooks.
[Ricochet spawn source](../upstream/hlsdk/ricochet/dlls/player.cpp)

The final probe SHA-256 was
`56395761cd9dbd13fc6055760cf9aa5ec41ec75674e7f8a527b9cab650a5b611`.
Run the same test with the following commands, transferring the built
probe to the audit host before invoking the run script.

```bash
bash verify/fake-client/build-linux-i686.sh /tmp/ricochet_probe.so stock
bash verify/fake-client/run-linux.sh /path/to/saved/installs \
  /path/to/ricochet_probe.so /path/to/new-stock-probe-run stock
```

### Implications for the safe API

The observed sequence supports designing a host- and GameDLL-scoped
operation that publishes a session only after setup succeeds and identity
is revalidated. The SDK needs an independent session generation and
should preserve engine allocation evidence across setup. The edict's
fake flag can be temporarily cleared by the GameDLL, and a reused edict
can retain its address and serial. The observed engine user IDs helped
distinguish occupants in this test, while the SDK should still track an
independent generation.

Model removal as an explicit request with a separate completion
observation. A disconnect pre-hook can invalidate SDK use of the session,
but it runs while the native teardown is still in progress. Avoid
automatic kick or edict removal on allocation or setup failure. The stock
GameDLL's unprepared disconnect path was deliberately left untested because
it assumes player private data. Generic safe creation, automatic rollback,
and immediate portable slot release remain unsupported by the evidence.

## Remaining verification

Win32 and other manager and GameDLL combinations need their own
observations. A transition before stock GameDLL private data exists remains untested. These tests are separate from compatibility certification.

## Rust SDK and Ricochet adapter

The game-agnostic SDK owns identity tracking and removal confirmation.
The separate `metamod-ricochet` workspace crate supplies stock Ricochet's
setup sequence. Opt in with `unsafe { Ricochet::new() }` only after
verifying the documented deployment contract. The unsafe opt-in is
deliberate because the engine ABI cannot establish which GameDLL setup
is valid. Once opted in, the adapter accepts a bounded ASCII name and
exposes a scoped creation method on the callback's server context.

Creation returns a `FakeClient` with a new `PlayerId` after setup and
identity validation. It does not synthesize admission or join callbacks.
Setup can fail after engine allocation. The SDK returns `SetupIncomplete`
and leaves the native slot alone because generic rollback is unsafe.
Dropping a Rust handle does not disconnect the player.

Call `Server::request_fake_client_removal` with the handle. This queues a
kick by numeric engine user ID and sets `RemovalQueued`. The disconnect
hook immediately removes the SDK's `PlayerId` from session lookup. The
handle reaches `Removed` only after the SDK sees that disconnect and a
later frame confirms that the slot no longer reports the original user ID.
A map change, shutdown or conflicting observed join can invalidate the
handle instead. Without confirmation the request can remain pending.
The host must not reassign a user ID while an outstanding command can
still act on it.

The [lifecycle example](../adapters/ricochet/examples/ricochet_lifecycle.rs)
passed on both isolated Linux i686 stock Ricochet configurations listed
above. Each run queued and confirmed two removals and observed distinct
`PlayerId` values for reuse of the same slot. The verified example binary
had SHA-256
`770fa1504814de73e95928bf61987c7111961250e3e296be6c62d68416ce6f45`.
Rust mocks additionally exercised disconnect during setup, allocation
returning after map retirement, a queued removal invalidated by a map
change, and a nested frame during confirmation. Win32 remains unverified.

```bash
cargo build --locked --release --target i686-unknown-linux-gnu \
  -p metamod-ricochet --example ricochet_lifecycle
```

Transfer `target/i686-unknown-linux-gnu/release/examples/libricochet_lifecycle.so`
and pass it to the same isolated `run-linux.sh` command in `stock` mode.
The example is a test plugin for the verified stock deployments.

## Live map change with a pending removal

The [map-change example](../adapters/ricochet/examples/ricochet_map_change.rs)
passed on **Valve HLDS build 8684** and **ReHLDS 3.15.0.896-dev**, both
Linux i686 with classic Metamod 1.21.1-am and stock Ricochet. Both servers
changed from `rc_arena` to `rc_deathmatch` and exited cleanly.

1. The probe created a fake client on the first map and observed
   `RemovalQueued`. The runner submitted the probe command and
   `changelevel` in one console line, placing the map command ahead of
   the kick appended by the probe.
2. Both engines delivered the old client's disconnect while the handle
   was still pending, before map deactivation. The native log identified
   the level change as the reason for the drop. At deactivation, the old
   handle became `Invalidated` and its `PlayerId` was no longer current.
3. The probe confirmed that the old handle could no longer request
   removal. Creation on the second map reused the slot with a distinct
   `PlayerId`. The replacement stayed current until its own queued kick,
   which the SDK independently observed and confirmed as `Removed`.

The runner checks the destination map in the server's `status` output
and corroborates the reason for each drop. The test covers a normal live
map transition while a kick is pending. The reentry case below exercises a
map change while the SDK creation call is still on the stack.

```bash
cargo build --locked --release --target i686-unknown-linux-gnu \
  -p metamod-ricochet --example ricochet_map_change
bash verify/fake-client/run-linux.sh /path/to/saved/installs \
  /path/to/libricochet_map_change.so /path/to/new-map-probe-run map
```

Transfer the built shared object to the audit host before running the
script. The tested binary SHA-256 was
`2cad8c4bf38cfe76016f739b904d1717362772d97edfb53dda2141d2f625e238`.

## Live reentry during SDK setup

The [setup-reentry example](../adapters/ricochet/examples/ricochet_setup_reentry.rs)
passed on Linux i686 Valve HLDS build 8684 and ReHLDS 3.15.0.896-dev,
both with classic Metamod 1.21.1-am and stock Ricochet.

The probe completed native Ricochet setup inside the SDK's setup closure.
Before returning from that closure, it synchronously executed
`changelevel rc_deathmatch` from `rc_arena`. Both engines dropped the
fake client as part of the level change. The closure reported success,
but the SDK detected the retired map and returned `SetupIncomplete`.
Neither client slot had a published `PlayerId` when creation returned
or on the following frame. The probe also checked that the SDK deferred
the nested map notifications, delivered one deactivation and a second
activation, and emitted no player join. Both runs confirmed the new map
in native console output and shut down cleanly.

The native player already had GameDLL private data when the test forced
the transition. Reentry before stock Ricochet has initialized that data
remains unverified on a live server. The example calls the native command
executor for the test without extending the SDK's safe API.

```bash
cargo build --locked --release --target i686-unknown-linux-gnu \
  -p metamod-ricochet --example ricochet_setup_reentry
bash verify/fake-client/run-linux.sh /path/to/saved/installs \
  /path/to/libricochet_setup_reentry.so /path/to/new-reentry-run reentry
```

Transfer the built shared object to the audit host before running the
script. The tested binary SHA-256 was
`7be2a1b81bfc143505fdf3ae310e8ff96926ed58197b188fa798484b7643836c`.
