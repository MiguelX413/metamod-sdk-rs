"""Relationship fixture selection without native engines or Steam assets."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "relationship_runner", Path(__file__).resolve().parents[1] / "entity-relationships/run-linux.py")
runner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runner)


class Console:
    def __init__(self, stream):
        self.stream = stream

    def write(self, data):
        for command, marker in [(b"prepare", b"LIVE"), (b"retire", b"RETIRED"), (b"reuse", b"REUSED")]:
            if b"sdk_relationship_" + command in data:
                self.stream.write(b"RELATIONSHIP " + marker + b" PASS\n")
        if b"changelevel" in data:
            self.stream.write(b"RELATIONSHIP MAP PASS\nRELATIONSHIP READY\n")
        self.stream.flush()

    def flush(self):
        pass

    def close(self):
        pass


class Process:
    def __init__(self, arguments, *, stdout, **kwargs):
        self.stdin = Console(stdout)
        self.returncode = None
        stdout.write(b"RELATIONSHIP READY\n")
        stdout.flush()

    def poll(self):
        return self.returncode

    def wait(self, timeout):
        self.returncode = 0
        return self.returncode


class RelationshipRunnerTests(unittest.TestCase):
    def test_complete_transcript_required_even_after_successful_waits(self):
        complete = b"".join(f"RELATIONSHIP {phase} PASS\n".encode() * count
                            for phase, count in (("LIVE", 2), ("RETIRED", 2), ("REUSED", 2), ("MAP", 1)))
        runner.validate_transcript(complete)
        for data in (complete + b"RELATIONSHIP FAIL late shutdown assertion\n",
                     complete + b"RELATIONSHIP LIVE PASS\n",
                     complete.replace(b"RELATIONSHIP MAP PASS\n", b"")):
            with self.subTest(data=data), self.assertRaises(RuntimeError):
                runner.validate_transcript(data)

    def test_manager_selection_and_default_engines(self):
        for manager, engines in [("classic", None), ("metamod-r", ["linux-rehlds"])]:
            with self.subTest(manager=manager), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                base = root / "installs"
                paths = ["hlds_linux", "engine_i486.so", "ricochet/dlls/ricochet.so",
                         "ricochet/addons/classic/dlls/metamod_i386.so",
                         "ricochet/addons/metamod-r/dlls/metamod_i386.so"]
                for engine in ("linux-hlds", "linux-rehlds"):
                    for relative in paths:
                        path = base / engine / relative
                        path.parent.mkdir(parents=True, exist_ok=True)
                        path.write_bytes(relative.encode())
                    (base / engine / "ricochet/liblist.gam").write_text('gamedll_linux "original.so"\n')
                plugin, fixture = root / "probe.so", root / "fixture.so"
                plugin.write_bytes(b"synthetic Rust plugin")
                fixture.write_bytes(b"synthetic native fixture")
                output = root / "run"
                with patch.object(runner.subprocess, "Popen", Process), patch.object(runner.time, "sleep"):
                    runner.run(base, plugin, fixture, output, engines=engines, metamod=manager)
                expected = ("linux-hlds", "linux-rehlds") if engines is None else engines
                for engine in expected:
                    relative = f"ricochet/addons/{manager}/dlls/metamod_i386.so"
                    metadata = json.loads((output / f"{engine}.launch.json").read_text())
                    self.assertEqual(metadata["sha256"][relative], hashlib.sha256(relative.encode()).hexdigest())
                    name = "metamod.so" if manager == "classic" else "metamod_i386.so"
                    self.assertIn(f'addons/{manager}/dlls/{name}',
                                  (output / engine / "ricochet/liblist.gam").read_text())
                    self.assertEqual((base / engine / "ricochet/liblist.gam").read_text(),
                                     'gamedll_linux "original.so"\n')
                    self.assertEqual((output / f"{engine}.exit").read_text(), "exit_status=0\n")
                if engines is not None:
                    self.assertFalse((output / "linux-hlds").exists())
                # A missing entry must not silently launch the source manager.
                (base / "linux-rehlds/ricochet/liblist.gam").write_text("game Ricochet\n")
                with patch.object(runner.subprocess, "Popen") as launch:
                    with self.assertRaisesRegex(ValueError, "one gamedll_linux entry"):
                        runner.run(base, plugin, fixture, root / "bad-config",
                                   engines=["linux-rehlds"], metamod=manager)
                    launch.assert_not_called()

    def test_metamod_r_requires_explicit_rehlds_selection(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with self.assertRaisesRegex(ValueError, "--engine linux-rehlds"):
                runner.run(root, root / "probe.so", root / "fixture.so", root / "out",
                           metamod="metamod-r")
            self.assertFalse((root / "out").exists())
