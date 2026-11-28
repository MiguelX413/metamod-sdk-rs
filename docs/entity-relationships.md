# Entity relationship live tests

On 2026-11-27, the relationship probe passed on three isolated Linux i686
configurations with stock Ricochet. All servers exited zero; no test servers
remained running. No clients were needed.

| Runtime | `rc_arena` | Target reuse | Map transition | `rc_deathmatch` |
| --- | --- | --- | --- | --- |
| Valve HLDS build 8684 + classic Metamod 1.21.1-am | Live links and vacant target passed | Slot 135, serial 0 → 1 | Old handles rejected | Live/vacant/reused checks passed; slot 87, serial 0 → 1 |
| ReHLDS 3.15.0.896-dev (`1963001`) + classic Metamod 1.21.1-am | Live links and vacant target passed | Slot 135, serial 0 → 1 | Old handles rejected | Live/vacant/reused checks passed; slot 87, serial 0 → 1 |
| ReHLDS 3.15.0.896-dev (`1963001`) + Metamod-R 1.3.0.149 (`603a257`) | Live links and vacant target passed | Slot 135, serial 0 → 1 | Old handles rejected | Live/vacant/reused checks passed; slot 87, serial 0 → 1 |

These are feature observations on the recorded binaries, not a new core v2
certificate or memory-safety certification. Runs were uninstrumented.
Win32, other GameDLLs, and forced native reentry during relationship
lookup were not tested here. Controlled Rust/Miri and native loader tests
retain their separate [memory-contract coverage](memory-safety-audit.md#entity-relationship-contracts).

## What the fixtures assert

The [native fixture](../verify/entity-relationships/fixture.cpp) creates two
`info_target` entities using the GameDLL's exported initializer. Stock
Ricochet provides this class. The SDK itself has no class-name dependency.
The [Rust probe](../examples/engine-demo/examples/entity_relationships.rs)
uses only safe SDK operations to check:

1. Owner and enemy identify the target, `aiment` identifies the source
   itself, and `ground_entity` identifies world index zero.
2. After native target removal, owner and enemy are absent and the saved
   target handle is no longer current. The native fixture leaves incoming
   relationship pointers unchanged.
3. After the engine's reuse delay, the fixture obtains the original target
   slot with a different serial. The snapshot identifies the new occupant,
   while the original target handle stays stale.
4. During deactivation and after the next activation, the old source handle
   is stale and cannot produce a snapshot. The saved target is stale on
   the new map as well.
5. Newly created entities pass the same live/removal/reuse sequence on the
   second map.

Each log contains two each of `LIVE PASS`, `RETIRED PASS`, and `REUSED PASS`,
plus one `MAP PASS`. These counts were checked independently after the live
runs. The current runner also checks the complete transcript after shutdown,
rejecting late failures, missing or duplicate results, and nonzero exits.
The native fixture bounds reuse attempts to 64 allocations, checks for spare
capacity, and releases temporary allocations. Map teardown owns remaining
test entities; no forced plugin unloading is used.

## Initial fixture finding

The first Valve run used raw `pfnCreateEntity` allocations. A native
diagnostic confirmed that the source and target had no private data and
that `pfnPEntityOfEntIndex` returned neither entity. The SDK correctly
reported the source unavailable. This was a fixture error, not a relationship
snapshot defect. The initial run and diagnostic both exited zero after the
assertion failure and remain preserved separately.

The successful fixture uses `pfnCreateNamedEntity("info_target")`, giving
the GameDLL an opportunity to initialize private data. Native diagnostics
confirmed that both entities were then visible. Discovery is limited to
what the engine's public lookup exposes; allocation alone does not establish
that an entity is discoverable.

## Evidence and build provenance

The classic Metamod evidence remains on the audit host under `~/entity-relationships.JFkDbD/`.
Successful evidence is in `run-v2/`, with per-engine logs, exit records,
launch arguments, and component hashes. `summary.json` records the checked
counts and hashes. The original `run/`, `diagnostic.log`, and
`allocation-diagnostic.log` retain the unsuccessful raw-allocation investigation.
The source installations were copied, not modified.

The SDK base revision was `3cc478d3f32137ed242fcea5fe65b66a83d12560` with no
changes to the SDK/raw/macro crates. The new Rust probe and native fixture
were uncommitted development sources: their build revisions are unknown,
not the base commit. `manifest-v2.json` records source and artifact hashes,
dirty state, Rust 1.100.1, GCC 16.2.1, and target `i686-unknown-linux-gnu`.

| Artifact | SHA-256 |
| --- | --- |
| Rust probe (`probe-v2.so`) | `587453385205dc181cd4b3f16307f2350ca18b343f1ed3a9079baf778305cd3b` |
| Native fixture (`fixture-v2.so`) | `18580546ca83c27e3561fc07a95d49bea9244a6d52f5eb714165ae7bc3ec9e3a` |
| Shared launcher | `f8a46f49c43ceb4a744bc4a415f4360f7fb93ac112a76616f84dd52c26168e18` |
| Valve engine module | `52e7ed23455eeb49a1561114fcde230b738cf94b40e2d77953555fb32ed07ba8` |
| ReHLDS engine module | `5bb5d862b7e65ced8c9cc5a2159d52d5522e3eb784ba0eb88e6624a410566e99` |
| Classic Metamod | `6956fb8be550189c827f14b919d998551c81b34e75becbb705c0b085d5390a20` |
| Stock Ricochet GameDLL | `57ea9f913b543d9fdfedcab43edfef8fcf5754fc9fada2bee6cac620eb4ce3e4` |
| Valve transcript | `75d73bfb022ff3586cb882806ae8ac3c7ff832b2c2d1ab1a6b9e8c89798d555c` |
| ReHLDS transcript | `9d74a136341dff0425253facd4567a6a98e2799d1a664de52f759574b46bfdc5` |
| Build manifest | `7fbcf9808d404a5a75fc2e461594f7355663f4b35b886aba7285f420f201ae61` |
| Executed runner (`run-v2.py`) | `88ad940579244bcefc2ef6e457bf4670221c731af5cebc74487e51af03b74ca5` |

### Metamod-R follow-up

The follow-up used a fresh copy of the saved ReHLDS installation with
Metamod-R 1.3.0.149. Both fixture binaries matched the classic runs byte for
byte. The manager changed; the launcher, engine and GameDLL hashes remained
the same. No compilation was performed on the audit host.

Evidence is under `~/entity-relationships-mmr.RgBaEM/`: `run/` contains the
transcript, launch metadata, component hashes and zero exit record;
`summary.json` records the independently checked marker counts and hashes.
`build-manifest.json` preserves the original development-artifact provenance.
The updated runner adds explicit engine/manager selection; its new hash is
separate from the earlier runner and unchanged probe build manifest.
The hashes below identify the executed copies; subsequent runner checks do
not change those historical artifacts or results.

| Artifact | SHA-256 |
| --- | --- |
| Metamod-R module | `8bed09c362f6d9019ce7d88b9a19d1ad08eca26bf46c0ed32604fbea2d7ba7a9` |
| ReHLDS/Metamod-R transcript | `07f5e8c6b199cae549591f831529147e7cdaff3ce1fba4efffe8176a6621c375` |
| Executed runner (`run-linux.py`) | `aaa13c244afcffa107b5c1307770b09baff946b2deab2e45be6a1f7e8b97fc1a` |

## Reproduce

On the development machine, initialize pinned submodules and install the
normal Linux i686 build prerequisites, then run:

```bash
bash verify/entity-relationships/build-linux-i686.sh target/relationship_fixture.so
sha256sum target/relationship_fixture.so \
  target/i686-unknown-linux-gnu/release/examples/libentity_relationships.so
```

Record the actual build revision, dirty state, toolchains, and source hashes
alongside new binaries. Transfer both shared libraries and
`verify/entity-relationships/run-linux.py` to the test host. With its existing
trusted snapshots and runtime dependencies, no Rust or C++ tools are needed:

```bash
python3 run-linux.py \
  "$HOME/ricochet-audit-lab/runs/20261122T115759Z-6890/installs" \
  ./libentity_relationships.so ./relationship_fixture.so \
  "$HOME/entity-relationships-new-run"
```

The output directory must not exist. Reserve port 27057 and enough disk for
two full server copies. The runner uses loopback-only servers, hyphen-free
Metamod config paths, and the supplied prebuilt fixtures. Do not connect
clients to this native entity-manipulation test. Preserve failed runs and
their binaries instead of replacing their logs with retry results.

To run only ReHLDS with Metamod-R, using one server copy:

```bash
python3 run-linux.py \
  "$HOME/ricochet-audit-lab/runs/20261122T115759Z-6890/installs" \
  ./libentity_relationships.so ./relationship_fixture.so \
  "$HOME/entity-relationships-mmr-new-run" \
  --engine linux-rehlds --metamod metamod-r
```

Defaults remain both engines with classic Metamod. This fixture runner rejects
Metamod-R with Valve HLDS; that combination is outside these recorded tests.
