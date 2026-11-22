"""Validate Linux i686 shared-library ELF headers."""

from pathlib import Path
import struct
import sys

ELF32_HEADER_SIZE = 52


def validate_shared_object(header, source):
    def require(condition, reason):
        if not condition:
            raise ValueError(f"{source}: {reason}")

    require(header[:4] == b"\x7fELF", "missing ELF magic")
    require(len(header) >= 16, "truncated ELF identification")
    require(header[4] == 1, f"expected ELF32 (class 1), found class {header[4]}")
    require(header[5] == 1, f"expected little-endian ELF (encoding 1), found encoding {header[5]}")
    require(len(header) >= ELF32_HEADER_SIZE, "truncated ELF32 header (need 52 bytes)")
    kind, machine, version = struct.unpack_from("<HHI", header, 16)
    require(kind == 3, f"expected ET_DYN shared object (type 3), found type {kind}")
    require(machine == 3, f"expected EM_386 (machine 3), found machine {machine}")
    require(header[6] == version == 1, "invalid ELF version")
    require(struct.unpack_from("<H", header, 40)[0] == ELF32_HEADER_SIZE, "invalid ELF32 header size")


if __name__ == "__main__":
    path = Path(sys.argv[1])
    try:
        with path.open("rb") as stream:
            validate_shared_object(stream.read(ELF32_HEADER_SIZE), path)
    except (OSError, ValueError) as error:
        sys.exit(str(error))
