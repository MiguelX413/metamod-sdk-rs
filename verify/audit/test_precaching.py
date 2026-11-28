"""Live precaching transcript validation using synthetic, non-host evidence."""
import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    "precache_log", Path(__file__).resolve().parents[1] / "precaching/check-log.py")
checker = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(checker)

TRANSCRIPT = """PRECACHE REGISTER PASS visit=1 model=17 sound=1 duplicates=equal
PRECACHE ACTIVATE PASS visit=1 model=17 sound=1
PRECACHE READY visit=1
PRECACHE STATUS PASS visit=1
PRECACHE DEACTIVATE PASS visit=1
PRECACHE REGISTER PASS visit=2 model=9 sound=1 duplicates=equal
PRECACHE STALE PASS old_model=17 new_model=9 old_sound=1 new_sound=1
PRECACHE ACTIVATE PASS visit=2 model=9 sound=1
PRECACHE READY visit=2
PRECACHE STATUS PASS visit=2
PRECACHE DEACTIVATE PASS visit=2
"""


class PrecachingTranscriptTests(unittest.TestCase):
    @unittest.skipUnless(sys.platform.startswith("linux"), "GNU/Linux fixture launcher")
    def test_launcher_selects_engines_and_manager(self):
        cases = [("precache", ("linux-hlds", "linux-rehlds"), "classic"),
                 ("precache-metamod-r", ("linux-rehlds",), "metamod-r")]
        for mode, engines, manager_name in cases:
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                base = root / "installs"
                # This shell stub is synthetic orchestration evidence, not an engine.
                lines = TRANSCRIPT.splitlines()
                stub = ("#!/usr/bin/env bash\nset -eu\nvisit=1\ncat <<'INITIAL'\n"
                        + "\n".join(lines[:3]) + "\nINITIAL\n"
                        + "while IFS= read -r command; do\ncase \"$command\" in\n"
                        + "sdk_precache_status) echo \"PRECACHE STATUS PASS visit=$visit\" ;;\n"
                        + "'changelevel rc_deathmatch') visit=2; cat <<'SECOND'\n"
                        + "\n".join(lines[4:9]) + "\nSECOND\n;;\n"
                        + "quit) echo 'PRECACHE DEACTIVATE PASS visit=2'; exit 0 ;;\n"
                        + "esac\ndone\n")
                for engine in ("linux-hlds", "linux-rehlds"):
                    source = base / engine
                    manager = source / f"ricochet/addons/{manager_name}/dlls/metamod_i386.so"
                    manager.parent.mkdir(parents=True)
                    manager.write_bytes(b"synthetic manager, never loaded")
                    (source / "ricochet/liblist.gam").write_text('gamedll_linux "original.so"\n')
                    executable = source / "hlds_linux"
                    executable.write_text(stub)
                    executable.chmod(0o755)
                plugin = root / "probe.so"
                plugin.write_bytes(b"synthetic probe, never loaded")
                output = root / "run"
                script = Path(__file__).resolve().parents[1] / "fake-client/run-linux.sh"
                result = subprocess.run(["bash", str(script), str(base), str(plugin), str(output), mode],
                                        capture_output=True, text=True, timeout=20)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual({p.name for p in output.iterdir() if p.is_dir()}, set(engines))
                for engine in engines:
                    filename = "metamod.so" if manager_name == "classic" else "metamod_i386.so"
                    self.assertIn(f'addons/{manager_name}/dlls/{filename}',
                                  (output / engine / "ricochet/liblist.gam").read_text())
                    self.assertEqual((base / engine / "ricochet/liblist.gam").read_text(),
                                     'gamedll_linux "original.so"\n')
                    self.assertEqual((output / f"{engine}.exit").read_text(), "exit_status=0\n")
                    checker.validate((output / f"{engine}.log").read_text())

    def test_complete_sequence_allows_numeric_index_reuse(self):
        self.assertEqual(checker.validate(TRANSCRIPT),
                         {"visits": 2, "first": [17, 1], "second": [9, 1]})

    def test_missing_duplicate_failed_or_reordered_records_are_rejected(self):
        for transcript in [TRANSCRIPT.replace("PRECACHE DEACTIVATE PASS visit=2\n", ""),
                           TRANSCRIPT + "PRECACHE STATUS PASS visit=2\n",
                           TRANSCRIPT + "PRECACHE FAIL late assertion\n",
                           TRANSCRIPT.replace("PRECACHE READY visit=1\n", "")
                           + "PRECACHE READY visit=1\n"]:
            with self.subTest(transcript=transcript), self.assertRaises(ValueError):
                checker.validate(transcript)

    def test_indices_must_be_positive_and_correlated(self):
        for transcript in [TRANSCRIPT.replace("model=17", "model=0"),
                           TRANSCRIPT.replace("old_model=17", "old_model=99"),
                           TRANSCRIPT.replace("ACTIVATE PASS visit=2 model=9", "ACTIVATE PASS visit=2 model=10")]:
            with self.subTest(transcript=transcript), self.assertRaises(ValueError):
                checker.validate(transcript)
