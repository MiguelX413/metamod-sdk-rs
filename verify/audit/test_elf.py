import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
import unittest
import zipfile

from elf import validate_shared_object


def header():
    return bytearray(struct.pack("<16sHHIIIIIHHHHHH", b"\x7fELF\x01\x01\x01",
                                 3, 3, 1, 0, 0, 0, 0, 52, 0, 0, 0, 0, 0))


class ElfTests(unittest.TestCase):
    def test_header_validation(self):
        validate_shared_object(header(), "fixture.so")
        for offset, value, reason in [
            (0, 0, "magic"), (4, 2, "ELF32"), (5, 2, "little-endian"),
            (16, 2, "ET_DYN"), (18, 62, "EM_386"), (20, 0, "version"), (40, 20, "header size"),
        ]:
            with self.subTest(offset=offset):
                data = header()
                data[offset] = value
                with self.assertRaisesRegex(ValueError, reason):
                    validate_shared_object(data, "fixture.so")
        for length in (4, 15, 20, 51):
            with self.subTest(length=length), self.assertRaisesRegex(ValueError, "truncated"):
                validate_shared_object(header()[:length], "fixture.so")

    @unittest.skipUnless(os.name == "posix" and shutil.which("bash"), "requires Bash on POSIX")
    def test_ubuntu_validation_and_extraction(self):
        script_dir = Path(__file__).resolve().parent
        script = (script_dir / "ricochet_audit_ubuntu.sh").read_text()
        functions = 'die() { echo "$*" >&2; exit 1; }\n' + "\n".join(
            name + "() {" + script.split(name + "() {", 1)[1].split("\n}", 1)[0] + "\n}"
            for name in ("validate_elf32", "extract_elf32", "stage_installation"))
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary = root / "plugin.so"
            binary.write_bytes(header())
            mock_file = root / "file"
            mock_file.write_text('#!/bin/sh\nprintf "%s\\n" "$FILE_DESCRIPTION"\n')
            mock_file.chmod(0o755)

            def invoke(command, *args, description="Intel i386"):
                return subprocess.run(["bash", "-c", functions + "\n" + command + ' "$@"', "_", *map(str, args)],
                                      env={**os.environ, "SCRIPT_DIR": str(script_dir),
                                           "PATH": str(root) + os.pathsep + os.environ["PATH"],
                                           "FILE_DESCRIPTION": description}, capture_output=True, text=True)

            for description in ("Intel i386", "Intel 80386", "localized output"):
                self.assertEqual(invoke("validate_elf32", binary, description=description).returncode, 0)
            archive = root / "release.zip"
            output = root / "extracted.so"
            with zipfile.ZipFile(archive, "w") as bundle:
                bundle.writestr("linux/plugin.so", binary.read_bytes())
            self.assertEqual(invoke("extract_elf32", archive, r"plugin\.so", output).returncode, 0)
            self.assertEqual(output.read_bytes(), binary.read_bytes())
            binary.write_bytes(header()[:20])
            rejected = invoke("validate_elf32", binary, description="ELF 32-bit shared object, Intel 80386")
            self.assertNotEqual(rejected.returncode, 0)
            self.assertIn("truncated ELF32 header", rejected.stderr)
            self.assertIn("Intel 80386", rejected.stderr)
            with zipfile.ZipFile(archive, "w") as bundle:
                bundle.writestr("linux/plugin.so", binary.read_bytes())
            rejected = invoke("extract_elf32", archive, r"plugin\.so", output)
            self.assertNotEqual(rejected.returncode, 0)
            self.assertIn("release.zip:linux/plugin.so: truncated", rejected.stderr)

            if all(shutil.which(cmd) for cmd in ("as", "ld", "readelf")) and sys.platform.startswith("linux"):
                source, destination, sdl = root / "steam", root / "stage", root / "system-sdl.so"
                source.mkdir()
                link = source / "libSDL2.so"
                link.symlink_to("libSDL2-2.0.so.0")
                (source / "server.cfg").write_text("fixture")
                (source / "config-link.cfg").symlink_to("server.cfg")
                obj = root / "fixture.o"
                subprocess.run(["as", "--32", "-o", str(obj)], input="", text=True, check=True)
                subprocess.run(["ld", "-m", "elf_i386", "-shared", "-soname", "libSDL2-2.0.so.0",
                                "-o", str(sdl), str(obj)], check=True)
                for stage in (destination, root / "stage-again"):
                    result = invoke("stage_installation", source, stage, sdl)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual((stage / "libSDL2.so").read_bytes(), sdl.read_bytes())
                    self.assertFalse((stage / "libSDL2.so").is_symlink())
                    self.assertFalse((stage / "config-link.cfg").is_symlink())
                    self.assertEqual((stage / "config-link.cfg").read_text(), "fixture")
                    self.assertTrue(link.is_symlink())

                link.unlink()
                link.symlink_to("/usr/lib/i386-linux-gnu/libSDL2-2.0.so.0")
                result = invoke("stage_installation", source, root / "system-alias", sdl)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual((root / "system-alias/libSDL2.so").read_bytes(), sdl.read_bytes())
                link.unlink()
                link.symlink_to("libSDL2-2.0.so.0")

                wrong_sdl = root / "wrong-sdl.so"
                subprocess.run(["ld", "-m", "elf_i386", "-shared", "-soname", "libunrelated.so",
                                "-o", str(wrong_sdl), str(obj)], check=True)
                result = invoke("stage_installation", source, root / "wrong-soname", wrong_sdl)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("lacks the SDL2 ABI SONAME", result.stderr)
                self.assertIn("libSDL2.so -> libSDL2-2.0.so.0", result.stderr)
                result = invoke("stage_installation", source, root / "missing-sdl", root / "absent-sdl.so")
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("Install libsdl2-2.0-0:i386", result.stderr)

                unexpected = source / "other.so"
                unexpected.symlink_to("absent.so")
                failed_stage = root / "unexpected"
                result = invoke("stage_installation", source, failed_stage, sdl)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("other.so -> absent.so", result.stderr)
                self.assertFalse(failed_stage.exists())
                unexpected.unlink()
                link.unlink()
                link.symlink_to("custom-sdl.so")
                result = invoke("stage_installation", source, failed_stage, sdl)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("libSDL2.so -> custom-sdl.so", result.stderr)
                self.assertFalse(failed_stage.exists())
