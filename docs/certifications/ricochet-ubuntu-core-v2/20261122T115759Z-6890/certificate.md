# Metamod SDK for Rust compatibility evaluation

Profile: **core v2**

Each verdict applies to the recorded binaries and tested itinerary. It is neither universal compatibility nor a memory-safety guarantee.

## linux-hlds-classic-stock — CERTIFIED

- Platform: `linux / i686`
- Engine: Valve HLDS 1.1.2.1
  - Launcher `hlds_linux` SHA-256: `f8a46f49c43ceb4a744bc4a415f4360f7fb93ac112a76616f84dd52c26168e18`
  - Engine module `engine_i486.so` SHA-256: `52e7ed23455eeb49a1561114fcde230b738cf94b40e2d77953555fb32ed07ba8`
- metamod: alliedmodders/metamod-hl1 1.21.1-am, SHA-256 `6956fb8be550189c827f14b919d998551c81b34e75becbb705c0b085d5390a20`
- gamedll: Stock Ricochet Steam app 90 steam_legacy, SHA-256 `57ea9f913b543d9fdfedcab43edfef8fcf5754fc9fada2bee6cac620eb4ce3e4`
- Audit plugin SHA-256: `bfdbf34ea5a66cefc191e824b9e6b3572ea054cd1aebc19328df6e4b545771b8`
- sdk_commit: `b9645b5bcf3940d28573881b2d12fbfa9be2142c`
- audit_plugin_commit: `b9645b5bcf3940d28573881b2d12fbfa9be2142c`
All 17 mandatory probes were corroborated.

## linux-hlds-classic-updated — CERTIFIED

- Platform: `linux / i686`
- Engine: Valve HLDS 1.1.2.1
  - Launcher `hlds_linux` SHA-256: `f8a46f49c43ceb4a744bc4a415f4360f7fb93ac112a76616f84dd52c26168e18`
  - Engine module `engine_i486.so` SHA-256: `52e7ed23455eeb49a1561114fcde230b738cf94b40e2d77953555fb32ed07ba8`
- metamod: alliedmodders/metamod-hl1 1.21.1-am, SHA-256 `6956fb8be550189c827f14b919d998551c81b34e75becbb705c0b085d5390a20`
- gamedll: ricochet-updated operator-supplied, SHA-256 `f38022611d7991d1666febc969569e40b29e0169bcc1b4aa93e3e947c5b9ced1`
- Audit plugin SHA-256: `bfdbf34ea5a66cefc191e824b9e6b3572ea054cd1aebc19328df6e4b545771b8`
- sdk_commit: `b9645b5bcf3940d28573881b2d12fbfa9be2142c`
- audit_plugin_commit: `b9645b5bcf3940d28573881b2d12fbfa9be2142c`
All 17 mandatory probes were corroborated.

## linux-rehlds-classic-stock — CERTIFIED

- Platform: `linux / i686`
- Engine: ReHLDS 3.15.0.896
  - Launcher `hlds_linux` SHA-256: `f8a46f49c43ceb4a744bc4a415f4360f7fb93ac112a76616f84dd52c26168e18`
  - Engine module `engine_i486.so` SHA-256: `5bb5d862b7e65ced8c9cc5a2159d52d5522e3eb784ba0eb88e6624a410566e99`
- metamod: alliedmodders/metamod-hl1 1.21.1-am, SHA-256 `6956fb8be550189c827f14b919d998551c81b34e75becbb705c0b085d5390a20`
- gamedll: Stock Ricochet Steam app 90 steam_legacy, SHA-256 `57ea9f913b543d9fdfedcab43edfef8fcf5754fc9fada2bee6cac620eb4ce3e4`
- Audit plugin SHA-256: `bfdbf34ea5a66cefc191e824b9e6b3572ea054cd1aebc19328df6e4b545771b8`
- sdk_commit: `b9645b5bcf3940d28573881b2d12fbfa9be2142c`
- audit_plugin_commit: `b9645b5bcf3940d28573881b2d12fbfa9be2142c`
All 17 mandatory probes were corroborated.

## linux-rehlds-classic-updated — CERTIFIED

- Platform: `linux / i686`
- Engine: ReHLDS 3.15.0.896
  - Launcher `hlds_linux` SHA-256: `f8a46f49c43ceb4a744bc4a415f4360f7fb93ac112a76616f84dd52c26168e18`
  - Engine module `engine_i486.so` SHA-256: `5bb5d862b7e65ced8c9cc5a2159d52d5522e3eb784ba0eb88e6624a410566e99`
- metamod: alliedmodders/metamod-hl1 1.21.1-am, SHA-256 `6956fb8be550189c827f14b919d998551c81b34e75becbb705c0b085d5390a20`
- gamedll: ricochet-updated operator-supplied, SHA-256 `f38022611d7991d1666febc969569e40b29e0169bcc1b4aa93e3e947c5b9ced1`
- Audit plugin SHA-256: `bfdbf34ea5a66cefc191e824b9e6b3572ea054cd1aebc19328df6e4b545771b8`
- sdk_commit: `b9645b5bcf3940d28573881b2d12fbfa9be2142c`
- audit_plugin_commit: `b9645b5bcf3940d28573881b2d12fbfa9be2142c`
All 17 mandatory probes were corroborated.

## linux-rehlds-metamod-r-stock — CERTIFIED

- Platform: `linux / i686`
- Engine: ReHLDS 3.15.0.896
  - Launcher `hlds_linux` SHA-256: `f8a46f49c43ceb4a744bc4a415f4360f7fb93ac112a76616f84dd52c26168e18`
  - Engine module `engine_i486.so` SHA-256: `5bb5d862b7e65ced8c9cc5a2159d52d5522e3eb784ba0eb88e6624a410566e99`
- metamod: Metamod-R 1.3.0.149, SHA-256 `8bed09c362f6d9019ce7d88b9a19d1ad08eca26bf46c0ed32604fbea2d7ba7a9`
- gamedll: Stock Ricochet Steam app 90 steam_legacy, SHA-256 `57ea9f913b543d9fdfedcab43edfef8fcf5754fc9fada2bee6cac620eb4ce3e4`
- Audit plugin SHA-256: `bfdbf34ea5a66cefc191e824b9e6b3572ea054cd1aebc19328df6e4b545771b8`
- sdk_commit: `b9645b5bcf3940d28573881b2d12fbfa9be2142c`
- audit_plugin_commit: `b9645b5bcf3940d28573881b2d12fbfa9be2142c`
All 17 mandatory probes were corroborated.

## linux-rehlds-metamod-r-updated — CERTIFIED

- Platform: `linux / i686`
- Engine: ReHLDS 3.15.0.896
  - Launcher `hlds_linux` SHA-256: `f8a46f49c43ceb4a744bc4a415f4360f7fb93ac112a76616f84dd52c26168e18`
  - Engine module `engine_i486.so` SHA-256: `5bb5d862b7e65ced8c9cc5a2159d52d5522e3eb784ba0eb88e6624a410566e99`
- metamod: Metamod-R 1.3.0.149, SHA-256 `8bed09c362f6d9019ce7d88b9a19d1ad08eca26bf46c0ed32604fbea2d7ba7a9`
- gamedll: ricochet-updated operator-supplied, SHA-256 `f38022611d7991d1666febc969569e40b29e0169bcc1b4aa93e3e947c5b9ced1`
- Audit plugin SHA-256: `bfdbf34ea5a66cefc191e824b9e6b3572ea054cd1aebc19328df6e4b545771b8`
- sdk_commit: `b9645b5bcf3940d28573881b2d12fbfa9be2142c`
- audit_plugin_commit: `b9645b5bcf3940d28573881b2d12fbfa9be2142c`
All 17 mandatory probes were corroborated.
