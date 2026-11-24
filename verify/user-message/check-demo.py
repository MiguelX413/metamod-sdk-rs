#!/usr/bin/env python3
"""Check the delivery fixture's registrations and exact wire bytes in a client demo."""
import hashlib
import struct
import sys
from pathlib import Path

path = Path(sys.argv[1])
if path.stat().st_size > 64 * 1024 * 1024:
    raise SystemExit("demo exceeds the fixture's 64 MiB read limit")
data = path.read_bytes()
if len(data) < 544 or data[:8] != b"HLDEMO\x00\x00" or struct.unpack_from("<i", data, 8)[0] != 5:
    raise SystemExit("expected a GoldSrc protocol-5 demo")


def registration(name, size):
    # svc_newusermsg, native ID, size (-1 means variable), 16-byte name.
    suffix = bytes([size & 255]) + name.ljust(16, b"\x00")
    matches = {data[i + 1] for i in range(len(data) - 18)
               if data[i] == 39 and data[i + 2:i + 19] == suffix}
    if len(matches) != 1:
        raise SystemExit(f"missing or ambiguous {name!r} registration")
    return matches.pop()


text_id = registration(b"TextMsg", -1)
fade_id = registration(b"ScreenFade", 10)
text = b"\x02SDK_MESSAGE_INTEGRITY_254_255_16909060\n" + bytes(5)
fade = bytes(6) + bytes([254, 255, 17, 0])
wire = bytes([text_id, len(text)]) + text + bytes([fade_id]) + fade
count = data.count(wire)
if count != 1:
    raise SystemExit(f"expected one exact adjacent payload pair, found {count}")
print(f"PASS TextMsg id={text_id} bytes={len(text)}; ScreenFade id={fade_id} bytes={len(fade)}")
print(f"demo_sha256={hashlib.sha256(data).hexdigest()}")
# This bounded byte check corroborates fixture payloads, without implementing
# a general demo decoder or establishing delivery for untested message schemas.
