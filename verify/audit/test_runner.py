import json
import mmap
import os
from pathlib import Path
import platform
import struct
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import runner

FIXTURES = Path(__file__).with_name("fixtures")


def wire(seq, kind, **fields):
    return runner.PREFIX + json.dumps({"schema_version": 1, "seq": seq, "kind": kind, **fields}).encode() + b"\n"


def result(seq, test, status):
    return runner.parse_event(wire(seq, "result", test=test, result=status, expected="expected", observed="evidence"))


def by_test(results):
    return {test["test"]: test for test in results}


class ProtocolTests(unittest.TestCase):
    def test_preserve_dumps_requires_crash_file_headers(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            stage = root / "stage"
            output = root / "reports"
            stage.mkdir()
            output.mkdir()
            elf32 = struct.pack("<16sHHIIIIIHHHHHH", b"\x7fELF\x01\x01\x01",
                                4, 3, 1, 0, 0, 0, 0, 52, 0, 0, 0, 0, 0)
            elf64 = struct.pack(">16sHHIQQQIHHHHHH", b"\x7fELF\x02\x02\x01",
                                4, 62, 1, 0, 0, 0, 0, 64, 0, 0, 0, 0, 0)
            mdmp = struct.pack("<4sIIIIIQIII", b"MDMP", 0xa793, 1, 32, 0, 0, 0, 0, 0, 0)
            genuine = {"core": elf32, "core.17": elf64, "crash.mdmp": mdmp}
            others = {"core.so": elf32[:16] + b"\x03" + elf32[17:], "core.wav": b"RIFF1234WAVE",
                      "truncated.core": elf32[:20], "broken.mdmp": mdmp[:32],
                      "unrelated.dmp": b"not a dump"}
            for name, data in (genuine | others).items():
                (stage / name).write_bytes(data)
            saved = runner.preserve_dumps(stage, output)
            self.assertEqual({name.split("-", 2)[2] for name in saved}, set(genuine))
            for name in saved:
                self.assertEqual((output / name).read_bytes(), genuine[name.split("-", 2)[2]])

    def test_template_covers_independent_components(self):
        matrix = runner.matrix_template(False)["runtimes"]
        self.assertEqual(len(matrix), 16)
        self.assertEqual(len({row["id"] for row in matrix}), 16)
        self.assertEqual(sum(row["experimental"] for row in matrix), 4)
        self.assertEqual(len(runner.matrix_template(True)["runtimes"]), 24)

    def test_parser_rejects_unknown_schema_and_malformed_records(self):
        self.assertIsNone(runner.parse_event(b'MM_AUDIT garbage'))
        self.assertIsNone(runner.parse_event(wire(1, "unknown")))
        self.assertIsNone(runner.parse_event(wire(True, "report")))
        self.assertIsNone(runner.parse_event(wire(4097, "report")))
        self.assertEqual(runner.parse_event(b"[server] " + wire(2, "report", complete=True))["seq"], 2)

    def test_out_of_order_streams_and_partial_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            a, b = folder / "out", folder / "err"
            a.write_bytes(wire(2, "report", complete=True) + b"MM_AUDIT {broken\n")
            b.write_bytes(wire(1, "status") + b"AddressSanitizer: crash\n")
            report = runner.aggregate(folder, {"id": "fixture"}, {"executed": True, "run_id": "run"}, [a, b])
            events = [json.loads(line) for line in (folder / "events.jsonl").read_text().splitlines()]
            self.assertEqual([event["seq"] for event in events], [1, 2])
            self.assertEqual(events[0]["run_id"], "run")
            self.assertFalse(report["protocol_complete"])
            self.assertTrue(report["diagnostics"])
            self.assertTrue(report["protocol_warnings"])

    def test_runtime_identity_needs_independent_evidence(self):
        runtime = {
            "os": "linux", "staged_root": "/stage",
            "components": {
                "engine": {"implementation": "Valve HLDS", "version": "1.1.2.7"},
                "metamod": {"implementation": "alliedmodders/metamod-hl1", "version": "1.21.1-am"},
                "gamedll": {"binary": "ricochet/dlls/updated.so"},
            },
        }
        lines = ["Exe version 1.1.2.7 (ricochet)", "Metamod v1.21.1-am",
                 "dll pathname: /stage/ricochet/dlls/updated.so"]
        mappings = {"engine": {"engine.so": True}, "metamod": {"manager.so": True}}
        events = [{"kind": "status"}]
        verified = runner.identity(runtime, lines, events, mappings)
        self.assertEqual(verified["engine"]["result"], "PASS")
        self.assertEqual(verified["metamod"]["result"], "PASS")
        self.assertEqual(verified["gamedll"]["result"], "PASS")
        self.assertEqual(verified["plugin"]["result"], "INCONCLUSIVE")
        listed = runner.identity(runtime, lines + ["[ 1] Rust Runtime Au  RUN  -  libmetamod_audit  v0.1.0 ini Start Never"], events, mappings)
        self.assertEqual(listed["plugin"]["result"], "PASS")
        contradicted = runner.identity(runtime, lines + ["ReHLDS version: 3.14.0.857",
                                       "Metamod-r version 1.3.0.149", "dll pathname: /elsewhere/updated.so"],
                                       events, mappings)
        for name in ("engine", "metamod", "gamedll"):
            self.assertEqual(contradicted[name]["result"], "FAIL")
        self.assertEqual(runner.identity(runtime, ["Exe version 1.1.2.8"], events, mappings)
                         ["engine"]["result"], "FAIL")

    def test_upstream_derived_console_formats(self):
        # Exercise the same filtering that runs before identity inference.
        def lines(*files):
            return runner.read_events([FIXTURES / file for file in files])[4]

        events = [{"kind": "status"}]
        mapped = {"engine": {"engine": True}, "metamod": {"manager": True}}
        classic = {"os": "linux", "staged_root": "/stage", "components": {
            "engine": {"implementation": "Valve HLDS", "version": "2013 build",
                       "console_version_field": "exe build", "console_version": "6153"},
            "metamod": {"implementation": "alliedmodders/metamod-hl1", "version": "1.21.1-am"},
            "gamedll": {"binary": "ricochet/dlls/ricochet.so"},
        }}
        old_lines = lines("hlds-captured.txt", "classic-synthetic.txt")
        self.assertEqual({x["result"] for x in runner.identity(classic, old_lines, events, mapped).values()}, {"PASS"})
        classic["components"]["engine"].update(console_version_field="exe version", console_version="1.1.2.2/Stdio")
        self.assertEqual(runner.identity(classic, old_lines, events, mapped)["engine"]["result"], "PASS")
        self.assertEqual(runner.identity(classic, old_lines, events, {})["engine"]["result"], "INCONCLUSIVE")
        noisy = ["say Rust Runtime Audit RUN",
                 " [2] Unrelated Plugin RUN - libmetamod_audit.so v1 ini Start Never",
                 " [3] Rust Runtime Audit RUN - libmetamod_audit.so.bak v1 ini Start Never"]
        self.assertEqual(runner.identity(classic, noisy, events, {})["plugin"]["result"], "INCONCLUSIVE")
        paused = [line.replace("  RUN  ", "  PAUS ") for line in old_lines]
        self.assertEqual(runner.identity(classic, paused, events, mapped)["plugin"]["result"], "FAIL")

        modern = {"os": "windows", "staged_root": r"c:\stage", "components": {
            "engine": {"implementation": "ReHLDS", "version": "3.14.0.857-dev"},
            "metamod": {"implementation": "Metamod-R", "version": "1.3.0.149"},
            "gamedll": {"binary": r"ricochet\dlls\ricochet_updated.dll"},
        }}
        new_lines = lines("rehlds-metamod-r-synthetic.txt")
        new_lines.append("Metamod-r version 14:20:03 Nov 20 2026")
        self.assertEqual({x["result"] for x in runner.identity(modern, new_lines, events, mapped).values()}, {"PASS"})
        self.assertEqual(runner.identity(classic, new_lines, events, mapped)["metamod"]["result"], "FAIL")

    def test_native_evidence_remains_separate_and_protocol_detects_loss(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            log = folder / "console"
            native = (b'MM_AUDIT_NATIVE {"schema_version":1,"test":"native.hook.result.v1",'
                      b'"result":"PASS","observed":"asserted fixture result"}\n')
            log.write_bytes(wire(1, "status") + wire(3, "suite_complete", suite="all")
                            + wire(4, "report", complete=True) + wire(4, "report", complete=True)
                            + b'MM_AUDIT {"seq":5' + b"\n" + native)
            common = {"executed": True, "run_id": "run"}
            real = runner.aggregate(folder, {"id": "real"}, common, [log])
            synthetic = runner.aggregate(folder, {"id": "fixture", "evidence_scope": "native-fixture"}, common, [log])
            self.assertFalse(real["protocol_complete"])
            self.assertTrue(any("duplicate" in warning for warning in real["protocol_warnings"]))
            self.assertTrue(any("missing" in warning for warning in real["protocol_warnings"]))
            self.assertFalse(real["native_fixture_results"])
            self.assertEqual(synthetic["native_fixture_results"][0]["source"], "native-harness")
            self.assertFalse(synthetic["runtime_identity"])

    def test_health_regressions_and_host_hash_drift(self):
        def report(**fields):
            return {"runs": [{"runtime": {"id": "host", "components": {"engine": {"sha256": fields.pop("sha", "a")}}},
                              "tests": [{"test": "same.v1", "result": "PASS"}], **fields}]}

        baseline = report(exit_status=0, protocol_complete=True)
        candidate = report(exit_status=-11, protocol_complete=False, sha="b",
                           sanitizer_diagnostics=["ERROR: AddressSanitizer"],
                           crash_diagnostics=["segmentation fault"],
                           truncated_streams=["stdout"], capture_errors=["pipe closed"])
        deltas = {row["field"]: row["category"] for row in runner.compare(baseline, candidate, [])}
        self.assertEqual(deltas["host_configuration"], "configuration drift")
        for field in ("exit_status", "protocol_complete", "sanitizer_diagnostics",
                      "crash_diagnostics", "truncated_streams", "capture_errors"):
            self.assertEqual(deltas[field], "regression")
        with self.assertRaises(ValueError):
            runner.compare(baseline, candidate, [{"reason": " "}])

    def test_sanitizer_evidence_survives_complete_protocol(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            log = folder / "log"
            log.write_bytes(wire(1, "ready", submitted=True) + wire(2, "status")
                            + wire(3, "result", test="init.v1", result="PASS", expected="init", observed="ran")
                            + wire(4, "suite_complete", suite="all") + wire(5, "report", complete=True)
                            + b"==42==ERROR: LeakSanitizer: detected memory leaks\n")
            run = runner.aggregate(folder, {"id": "runtime"}, {"executed": True, "run_id": "run"}, [log])
            self.assertTrue(run["protocol_complete"])
            self.assertEqual(by_test(run["tests"])["init.v1"]["result"], "PASS")
            self.assertEqual(len(run["sanitizer_diagnostics"]), 1)

    def test_conflicting_duplicate_does_not_erase_failure_or_later_diagnostics(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            log = folder / "log"
            log.write_bytes(wire(1, "result", test="init.v1", result="PASS", expected="init", observed="first")
                            + wire(1, "result", test="init.v1", result="FAIL", expected="init", observed="conflict")
                            + b"segmentation fault\n")
            run = runner.aggregate(folder, {"id": "runtime"}, {"executed": True, "run_id": "run"}, [log])
            self.assertFalse(run["protocol_complete"])
            self.assertEqual(by_test(run["tests"])["init.v1"]["result"], "FAIL")
            self.assertEqual(run["crash_diagnostics"], ["segmentation fault"])

    def test_missing_and_manual_evidence_do_not_hide_failure(self):
        expected = [
            result(1, "init.v1", "PASS"),
            result(2, "map.activate.v1", "FAIL"),
            result(3, "map.activate.v1", "PASS"),
        ]
        manual = [
            {"test": "map.activate.v1", "result": "PASS", "observed": "manual observation"},
            {"test": "hook.result.v1", "result": "PASS", "observed": "isolated fixture"},
        ]
        tests = by_test(runner.findings(expected, True, manual))
        self.assertEqual(tests["map.activate.v1"]["result"], "FAIL")
        self.assertEqual(tests["map.activate.v1"]["event_sequences"], [2, 3])
        self.assertEqual(tests["map.activate.v1"]["manual_conflict"], "PASS")
        self.assertEqual(tests["hook.result.v1"]["source"], "manual")
        self.assertEqual(tests["shutdown.v1"]["result"], "SKIP")
        self.assertEqual(by_test(runner.findings([], False, []))["init.v1"]["observed"], "runtime did not execute")

    def test_comparison_separates_regression_coverage_and_allowance(self):
        def report(status):
            return {"runs": [{"runtime": {"id": "fixture"}, "tests": [{"test": "a.v1", "result": status}]}]}

        baseline = report("PASS")
        self.assertEqual(runner.compare(baseline, report("FAIL"), [])[0]["category"], "regression")
        self.assertEqual(runner.compare(baseline, report("SKIP"), [])[0]["category"], "coverage loss")
        allowance = {"runtime": "fixture", "test": "a.v1", "before": "PASS", "after": "SKIP", "reason": "fixture disabled"}
        annotated = runner.compare(baseline, report("SKIP"), [allowance])[0]
        self.assertEqual((annotated["category"], annotated["severity"]), ("expected", "coverage loss"))

    def test_comparison_cli_fails_through_drift_and_allowance(self):
        def run(name, sha, status):
            return {"runtime": {"id": name, "components": {"engine": {"sha256": sha}}},
                    "tests": [{"test": "a.v1", "result": status, "observed": "raw evidence"}]}

        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            baseline, candidate, rules = (folder / name for name in ("baseline.json", "candidate.json", "rules.json"))
            runner.dump(baseline, {"runs": [run("host", "old", "PASS")]})
            runner.dump(candidate, {"runs": [run("host", "new", "FAIL")]})
            command = [sys.executable, runner.__file__, "compare", str(baseline), str(candidate)]
            unfiltered = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(unfiltered.returncode, 1, unfiltered.stderr)
            self.assertEqual(json.loads(unfiltered.stdout)[-1]["category"], "incomparable probe change")
            runner.dump(rules, [{"runtime": "host", "test": "a.v1", "before": "PASS", "after": "FAIL",
                                 "reason": "different vendor build"}])
            annotated = subprocess.run(command + ["--allowances", str(rules)], capture_output=True, text=True)
            self.assertEqual(annotated.returncode, 1, annotated.stderr)
            change = json.loads(annotated.stdout)[-1]
            self.assertEqual((change["category"], change["severity"]), ("expected", "failure"))
            self.assertEqual(change["comparability"], "configuration changed")
            self.assertEqual(change["after_evidence"], "raw evidence")
            # Matrix failure must be actionable even without a right-hand candidate.
            runner.dump(candidate, {"runs": [run("failing", "new", "FAIL"), run("passing", "old", "PASS")]})
            matrix = subprocess.run(command[:-2] + [str(candidate)], capture_output=True, text=True)
            self.assertEqual(matrix.returncode, 1, matrix.stderr)
            self.assertTrue(any(row.get("runtime") == "failing" and row.get("severity") == "failure"
                                for row in json.loads(matrix.stdout)))
            runner.dump(baseline, {"runs": [{**run("host", "old", "PASS"), "protocol_complete": True, "exit_status": 0}]})
            runner.dump(candidate, {"runs": [{**run("host", "old", "PASS"), "protocol_complete": False, "exit_status": -11,
                                               "sanitizer_diagnostics": ["ERROR: AddressSanitizer: fault"]}]})
            unhealthy = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(unhealthy.returncode, 1, unhealthy.stderr)
            fields = {row["field"] for row in json.loads(unhealthy.stdout) if row["severity"] == "failure"}
            self.assertEqual(fields, {"protocol_complete", "exit_status", "sanitizer_diagnostics"})

    def bind_native(self, folder, logs):
        import uuid
        import native_identity
        artifacts = folder / 'artifacts'
        artifacts.mkdir(exist_ok=True)
        for name in native_identity.ARTIFACTS:
            (artifacts / name).write_bytes(name.encode())
        build = dict(schema_version=1, run_id=str(uuid.uuid4()), target='i686-unknown-linux-gnu',
                     artifacts={name: runner.digest(artifacts / name) for name in native_identity.ARTIFACTS},
                     revisions={name: dict(commit='a' * 40, status='', diff_sha256='b' * 64)
                                for name in ('.', 'upstream/metamod-hl1', 'upstream/hlsdk')},
                     toolchain=dict(rust='rustc test', cxx='c++ test'),
                     sanitizers=dict(rust_asan=True, loader_asan_ubsan_lsan=True), environment={})
        runner.dump(folder / 'build.json', build)
        with logs[0].open('a') as stream:
            stream.write(native_identity.PREFIX + json.dumps(build) + '\n')
        runtime = dict(id='fixture', os='linux', architecture='i686', exit_status=0,
                       native_identity=dict(run_id=build['run_id'], build_sha256=runner.digest(folder / 'build.json'),
                                            logs={log.name: runner.digest(log) for log in logs}))
        runner.dump(folder / 'runtime.json', runtime)
        return runtime

    def test_native_capture_rejects_source_change_during_build(self):
        import native_identity
        from unittest.mock import patch
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            work = folder / 'work'
            output = folder / 'evidence'
            output.mkdir()
            (work / 'target/i686-unknown-linux-gnu/release').mkdir(parents=True)
            (work / 'load').write_bytes(b'loader')
            for name in native_identity.ARTIFACTS - {'loader'}:
                (work / 'target/i686-unknown-linux-gnu/release' /
                 ('lib' + name.replace('-', '_') + '.so')).write_bytes(b'plugin')
            (output / 'source-before.json').write_text('{}')
            with patch.object(native_identity, 'source_identity', return_value={'changed': True}):
                with self.assertRaisesRegex(ValueError, 'source state changed'):
                    native_identity.capture(output, work)
            self.assertFalse((output / 'build.json').exists())

    def test_native_identity_rejects_mixed_evidence(self):
        import native_identity
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            log = folder / 'loader.log'
            log.write_bytes(b'fixture transcript\n\xff\n')
            runtime = self.bind_native(folder, [log])
            self.assertEqual(native_identity.validate(runtime, [log], folder)['target'],
                             'i686-unknown-linux-gnu')
            for name in ('run', 'target', 'manifest', 'artifact', 'log', 'missing-log', 'marker'):
                with self.subTest(name=name):
                    log.write_text('fixture transcript\n')
                    runtime = self.bind_native(folder, [log])
                    if name == 'run':
                        runtime['native_identity']['run_id'] = 'other'
                    elif name == 'target':
                        runtime['architecture'] = 'x86_64'
                    elif name == 'manifest':
                        (folder / 'build.json').write_text('{}')
                    elif name == 'artifact':
                        (folder / 'artifacts/loader').write_bytes(b'other build')
                    elif name == 'log':
                        log.write_text('other run')
                    elif name == 'marker':
                        log.write_text('no build marker\n')
                        runtime['native_identity']['logs']['loader.log'] = runner.digest(log)
                    with self.assertRaises((ValueError, KeyError)):
                        native_identity.validate(runtime, [] if name == 'missing-log' else [log], folder)

    def test_strict_native_import_requires_complete_unique_evidence(self):
        def native(test, observed="assertion completed"):
            return runner.NATIVE_PREFIX + json.dumps({"schema_version": 1, "test": test, "result": "PASS",
                                                      "observed": observed}).encode() + b"\n"

        plugin = (wire(1, "ready", submitted=True) + wire(2, "status") + wire(3, "suite_complete", suite="all")
                  + wire(4, "report", complete=True))
        shutdown = b"".join(wire(seq, "result", test=test, result="PASS", expected="shutdown", observed="ran")
                            for seq, test in ((5, "shutdown.v1"), (6, "shutdown.drop.v1")))
        records = [native(test) for test in sorted(runner.NATIVE_SUITE)]
        cases = [
            ("complete", b"".join(records), shutdown, 0),
            ("missing", b"".join(records[:-1]), shutdown, 1),
            ("missing-retained-cvar", b"".join(native(test) for test in sorted(runner.NATIVE_SUITE)
                                              if test != "native.cvar.retained_storage.v1"), shutdown, 1),
            ("duplicate-retained-cvar", b"".join(records) + native("native.cvar.retained_storage.v1"), shutdown, 1),
            ("duplicate", b"".join(records + records[:1]), shutdown, 1),
            ("conflicting", b"".join(records) + native(sorted(runner.NATIVE_SUITE)[0], "different assertion"), shutdown, 1),
            ("unexpected", b"".join(records) + native("native.other.v1"), shutdown, 1),
            ("malformed", b"".join(records) + b"MM_AUDIT_NATIVE {broken\n", shutdown, 1),
            ("missing-reentrant-string", b"".join(native(test) for test in sorted(runner.NATIVE_SUITE)
                                                  if test != "native.string.reentrant_invalidation.v1"), shutdown, 1),
            ("duplicate-reentrant-string", b"".join(records) + native("native.string.reentrant_invalidation.v1"), shutdown, 1),
            ("missing-retired-arena", b"".join(native(test) for test in sorted(runner.NATIVE_SUITE)
                                              if test != "native.entity.retired_arena.v1"), shutdown, 1),
            ("duplicate-retired-arena", b"".join(records) + native("native.entity.retired_arena.v1"), shutdown, 1),
            ("missing-shutdown", b"".join(records), b"", 1),
            ("reversed-shutdown", b"".join(records),
             wire(5, "result", test="shutdown.drop.v1", result="PASS", expected="shutdown", observed="ran")
             + wire(6, "result", test="shutdown.v1", result="PASS", expected="shutdown", observed="ran"), 1),
            ("sanitizer-after-report", b"".join(records), shutdown + b"ERROR: AddressSanitizer: fault\n", 1),
        ]
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            runner.dump(folder / "runtime.json", {"id": "fixture", "exit_status": 0})
            for name, assertions, stderr, expected_exit in cases:
                with self.subTest(name=name):
                    (folder / "out.log").write_bytes(plugin + assertions)
                    (folder / "err.log").write_bytes(stderr)
                    self.bind_native(folder, [folder / "err.log", folder / "out.log"])
                    proc = subprocess.run([sys.executable, runner.__file__, "import", str(folder / "runtime.json"),
                                           str(folder / name), str(folder / "err.log"), str(folder / "out.log"),
                                           "--native-fixture", "--require-native-suite"], capture_output=True, text=True)
                    self.assertEqual(proc.returncode, expected_exit, proc.stderr)
                    report = runner.load(folder / name / "report.json")["runs"][0]
                    self.assertEqual(report["evidence_scope"], "native-fixture")
                    if name == "complete":
                        self.assertTrue(report["protocol_complete"])
                        self.assertEqual(set(by_test(report["native_fixture_results"])), runner.NATIVE_SUITE)

    def test_unknown_probe_cannot_be_aggregated_as_pass(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            log = folder / "log"
            log.write_bytes(wire(1, "result", test="invented.v1", result="PASS", expected="anything", observed="claimed"))
            run = runner.aggregate(folder, {"id": "runtime"}, {"executed": True, "run_id": "run"}, [log])
            self.assertNotIn("invented.v1", by_test(run["tests"]))
            self.assertIn("unknown probe identifier", run["protocol_warnings"][0])
            self.assertIn("invented.v1", (folder / "events.jsonl").read_text())


@unittest.skipUnless(platform.system() == "Windows", "Windows Toolhelp module enumeration")
class WindowsMappingTests(unittest.TestCase):
    def test_module_snapshot_matches_executable_path(self):
        executable = Path(sys.executable).resolve(strict=True)
        mapped = runner.windows_mappings(os.getpid(), {"engine": [executable]})
        self.assertTrue(mapped["engine"][str(executable)])


@unittest.skipUnless(platform.system() == "Linux", "fake launcher uses a POSIX shebang")
class ExecutionTests(unittest.TestCase):
    def test_valve_hlds_stages_alias_and_legacy_safe_configuration(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source"
            mod = source / "ricochet"
            manager = mod / "addons/classic/dlls/metamod_i386.so"
            manager.parent.mkdir(parents=True)
            manager.write_bytes(b"classic manager")
            (mod / "maps").mkdir()
            (mod / "maps/rc_arena.bsp").touch()
            original = b'gamedll_linux "stock.so"\n'
            (mod / "liblist.gam").write_bytes(original)
            (mod / "stock.so").touch()
            (source / "hlds_linux").touch()
            plugin = root / "libmetamod_audit.so"
            plugin.write_bytes(b"prebuilt plugin")
            spec = {
                "id": "fixture", "os": "linux", "installation": "source", "mod": "ricochet",
                "maps": ["rc_arena"], "plugin": plugin.name, "plugin_sha256": runner.digest(plugin),
                "engine": {"implementation": "Valve HLDS", "version": "8684", "binary": "hlds_linux"},
                "metamod": {"implementation": "alliedmodders/metamod-hl1", "version": "1.21.1-am",
                            "binary": manager.relative_to(source).as_posix()},
                "gamedll": {"implementation": "Stock Ricochet", "version": "1", "binary": "ricochet/stock.so"},
            }
            for engine, expected_name, config_dir in (
                ("Valve HLDS", "metamod.so", "mm_audit"),
                ("ReHLDS", "metamod_i386.so", "metamod-audit"),
            ):
                output = root / engine
                output.mkdir()
                with patch.object(runner, "check_binary"):
                    record = runner.run_one({**spec, "engine": {**spec["engine"], "implementation": engine}},
                                            root, output, "fixture-run", False, 0, prepare_only=True)
                self.assertEqual(record["execution"], "PREPARED", record)
                launch = runner.load(output / "launch.json")
                stage = Path(launch["cwd"])
                staged_mod = stage / "ricochet"
                self.assertIn(f'addons/classic/dlls/{expected_name}', (staged_mod / "liblist.gam").read_text())
                manager_identity = record["runtime"]["components"]["metamod"]
                loaded = stage / manager_identity.get("loaded_binary", manager_identity["binary"])
                self.assertEqual(loaded.name, expected_name)
                self.assertEqual(runner.digest(loaded), manager_identity["sha256"])
                self.assertFalse(loaded.is_symlink())
                if engine == "ReHLDS":
                    self.assertFalse(loaded.with_name("metamod.so").exists())
                for option, filename in (("mm_configfile", "config.ini"), ("mm_pluginsfile", "plugins.ini")):
                    self.assertEqual(launch["argv"][launch["argv"].index(option) + 1],
                                     f"addons/{config_dir}/{filename}")
                plugins = staged_mod / f"addons/{config_dir}/plugins.ini"
                self.assertEqual(plugins.read_text(), "linux addons/metamod-audit/libmetamod_audit.so\n")
                self.assertEqual(runner.digest(staged_mod / "addons/metamod-audit" / plugin.name),
                                 spec["plugin_sha256"])
            self.assertEqual((mod / "liblist.gam").read_bytes(), original)
            self.assertFalse(manager.with_name("metamod.so").exists())
            self.assertFalse((mod / "addons/mm_audit").exists())
            self.assertFalse((mod / "addons/metamod-audit").exists())
            alias = manager.with_name("metamod.so")
            alias.write_bytes(b"different manager")
            output = root / "conflict"
            output.mkdir()
            with patch.object(runner, "check_binary"):
                record = runner.run_one(spec, root, output, "fixture-run", False, 0, prepare_only=True)
            self.assertEqual(record["execution"], "UNAVAILABLE")
            self.assertIn("Metamod alias conflicts", record["error"])
            self.assertEqual(alias.read_bytes(), b"different manager")

    def test_linux_mappings_use_file_identity(self):
        executable = Path("/proc/self/exe").resolve(strict=True)
        with tempfile.NamedTemporaryFile() as data:
            other = Path(data.name)
            data.write(b"x")
            data.flush()
            with mmap.mmap(data.fileno(), 1):
                mapped = runner.linux_mappings(os.getpid(), {"engine": [executable], "plugin": [other]})
        self.assertTrue(mapped["engine"][str(executable)])
        self.assertFalse(mapped["plugin"][str(other)])

    def test_isolated_stage_process_and_capture(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source"
            mod = source / "ricochet"
            (mod / "maps").mkdir(parents=True)
            for map_name in ("rc_arena", "rc_deathmatch"):
                (mod / "maps" / f"{map_name}.bsp").touch()
            liblist = mod / "liblist.gam"
            original = b'gamedll "dlls/ricochet.dll"\r\ngamedll_linux "stock.so"\r\n'
            liblist.write_bytes(original)
            original_plugins = mod / "plugins.ini"
            original_plugins.write_text("original")
            for name in ("meta.so", "stock.so"):
                (mod / name).touch()
            (root / "libmetamod_audit.so").touch()
            engine = source / "hlds_linux"
            engine.write_text("""#!/usr/bin/env python3
import json
import sys
seq = 0
def emit(kind, **fields):
    global seq
    seq += 1
    print("MM_AUDIT " + json.dumps(dict(schema_version=1, seq=seq, kind=kind, **fields)), flush=True)
assert b'gamedll_linux "meta.so"' in open("ricochet/liblist.gam", "rb").read()
assert open("ricochet/plugins.ini").read() == "original"
emit("ready", submitted=True)
for line in sys.stdin:
    if line.startswith("version"):
        print("Exe version fixture 1.0", flush=True)
    elif line.startswith("mm_audit_status"):
        emit("status")
    elif line.startswith("changelevel"):
        emit("result", test="map.deactivate.v1", result="PASS", expected="deactivation", observed="fake callback")
        emit("result", test="map.activate.v1", result="PASS", expected="activation", observed="fake callback")
    elif line == 'mm_audit_run all "audit two words"\\n':
        emit("suite_complete", suite="all")
    elif line.startswith("mm_audit_report"):
        emit("report", complete=True)
    elif line.startswith("quit"):
        emit("result", test="shutdown.v1", result="PASS", expected="shutdown", observed="fake callback")
        break
""")
            engine.chmod(0o755)
            output = root / "report"
            output.mkdir()
            spec = {
                "id": "fake", "os": "linux", "installation": "source", "mod": "ricochet",
                "maps": ["rc_arena", "rc_deathmatch"], "map_cycles": 2, "timeout_seconds": 2,
                "plugin": "libmetamod_audit.so",
                "engine": {"implementation": "fake", "version": "1", "binary": "hlds_linux"},
                "metamod": {"implementation": "fake", "version": "2", "binary": "ricochet/meta.so"},
                "gamedll": {"implementation": "fake", "version": "3", "binary": "ricochet/stock.so"},
            }
            # The fixture uses a script and placeholder libraries. Keep the
            # real process, staging, stdin and report path under test.
            with patch.object(runner, "check_binary"):
                record = runner.run_one(spec, root, output, "fixture-run", False, 0)
                missing = runner.run_one({**spec, "installation": "missing"}, root, output, "fixture-run", False, 0)
                unsupported = runner.run_one({**spec, "unsupported_reason": "fixture exclusion"}, root, output,
                                             "fixture-run", False, 0)
                manual_output = root / "manual"
                manual_output.mkdir()
                prepared = runner.run_one(spec, root, manual_output, "fixture-run", False, 0, prepare_only=True)
            self.assertEqual(record["execution"], "COMPLETE", record)
            self.assertEqual(record["exit_status"], 0)
            self.assertEqual([t["result"] for t in record["map_transitions"]], ["PASS"] * 3)
            self.assertEqual(by_test(record["tests"])["shutdown.v1"]["result"], "PASS")
            self.assertEqual(record["console_version_observations"], ["Exe version fixture 1.0"])
            self.assertEqual(liblist.read_bytes(), original)
            self.assertEqual(original_plugins.read_text(), "original")
            self.assertFalse(list(output.glob("goldsrc-audit-*")))
            self.assertEqual(missing["execution"], "UNAVAILABLE")
            self.assertFalse(missing["executed"])
            self.assertEqual(unsupported["execution"], "UNSUPPORTED")
            self.assertEqual(by_test(unsupported["tests"])["init.v1"]["classification"], "Unsupported configuration")
            self.assertEqual(prepared["execution"], "PREPARED")
            self.assertFalse(prepared["executed"])
            launch = json.loads((manual_output / "launch.json").read_text())
            self.assertIn("-condebug", launch["argv"])
            self.assertTrue((Path(launch["cwd"]) / "ricochet/addons/metamod-audit/plugins.ini").exists())

            crash_output = root / "crash"
            crash_output.mkdir()
            engine.write_text(engine.read_text().replace('emit("report", complete=True)', 'emit("report", complete=True); sys.exit(19)'))
            with patch.object(runner, "check_binary"):
                crash = runner.run_one(spec, root, crash_output, "fixture-run", False, 0)
            self.assertEqual(crash["execution"], "INCOMPLETE")
            self.assertEqual(crash["exit_status"], 19)
            self.assertTrue(crash["protocol_complete"])
            self.assertIn('\"kind\": \"ready\"', (crash_output / "events.jsonl").read_text())

            interrupted_output = root / "interrupted-map"
            interrupted_output.mkdir()
            line = 'emit("result", test="map.activate.v1", result="PASS", expected="activation", observed="fake callback")'
            engine.write_text(engine.read_text().replace(line, line + "; sys.exit(20)"))
            with patch.object(runner, "check_binary"):
                interrupted = runner.run_one(spec, root, interrupted_output, "fixture-run", False, 0)
            self.assertEqual(interrupted["exit_status"], 20)
            self.assertEqual(interrupted["execution"], "INCOMPLETE")
            transition, = interrupted["map_transitions"]
            self.assertTrue(transition["deactivated"])
            self.assertTrue(transition["callback_sequences"])
            self.assertEqual(transition["result"], "INCONCLUSIVE")


if __name__ == "__main__":
    unittest.main()
