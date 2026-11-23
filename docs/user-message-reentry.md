# GoldSrc user-message reentry

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
- Keep native sending outside the safe API until a supported host
  provides an enforceable transaction or an explicit integration
  contract. Do not use native `MessageEnd` as automatic unwind cleanup
  when ownership may already have changed.

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
Win32, other managers, recipient delivery, and map transitions during
an open message remain unverified.
