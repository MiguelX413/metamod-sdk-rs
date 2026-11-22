import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

import certification
import runner


class CertificationTests(unittest.TestCase):
    def setUp(self):
        work = tempfile.TemporaryDirectory()
        self.addCleanup(work.cleanup)
        self.root = Path(work.name)
        self.report = self.root / "report.json"

    def collect(self, runtime_id="live", profile="core", omit=(), failures=(), manual=(), windows=False, later_omit=(), altered=None):
        if windows:
            system, arch, target, stage = "windows", "x86", "i686-pc-windows-msvc", r"C:\stage"
        else:
            system, arch, target, stage = "linux", "i686", "i686-unknown-linux-gnu", "/stage"
        runtime = {
            "id": runtime_id, "os": system, "architecture": arch, "staged_root": stage,
            "host": {"system": system.title(), "release": "1.2", "version": "build 3", "machine": "x86_64"},
            "components": {
                "engine": {"implementation": "ReHLDS", "version": "3.15.0.896", "binary": "engine",
                           "console_version": "3.15.0.896-dev", "sha256": "a" * 64,
                           "modules": {"engine-module": "e" * 64}},
                "metamod": {"implementation": "Metamod-R", "version": "1.3.0", "binary": "metamod",
                            "sha256": "b" * 64},
                "gamedll": {"implementation": "GameDLL", "version": "1.0", "binary": "game",
                            "sha256": "c" * 64},
            },
            "runner_revision": {"commit": "a" * 40, "working_tree_dirty": False},
            "plugin": {"sha256": "d" * 64, "target": target, "artifact_manifest": {
                "schema_version": 1, "sha256": "d" * 64, "working_tree_dirty": False,
                "audit_plugin_commit": "b" * 40, "sdk_commit": "c" * 40,
            }},
            "map_itinerary": ["map_one", "map_two"],
        }
        events = []
        catalog = runner.catalog()

        def emit(kind, **fields):
            events.append({"schema_version": 1, "seq": len(events) + 1, "kind": kind, **fields})
            return events[-1]["seq"]

        def probe(test):
            if test not in omit:
                emit("result", test=test, result="FAIL" if test in failures else "PASS",
                     expected="weaker contract" if test == altered else catalog[test], observed="fixture observation")

        probe("init.v1")
        probe("map.activate.v1")
        emit("ready", submitted=True)
        emit("status")
        special = runner.MAP_PROBES | {"init.v1", "shutdown.v1", "shutdown.drop.v1"}
        suite_start = events[-1]["seq"]
        for test in sorted(certification.PROFILES[profile] - special):
            probe(test)
        first_suite = emit("suite_complete", suite="all")
        first = len(events) + 1
        for test in ("map.deactivate.v1", "map.stale.v1", "map.activate.v1", "map.order.v1"):
            probe(test)
        last = len(events)
        for test in sorted(certification.MAP_SUITE_PROBES - set(later_omit)):
            probe(test)
        second_suite = emit("suite_complete", suite="all")
        emit("report", complete=True)
        probe("shutdown.v1")
        probe("shutdown.drop.v1")

        output = self.root / runtime_id
        output.mkdir()
        log = output / "stdout.log"
        log.write_text("ReHLDS version: 3.15.0.896-dev\nMetamod-R version: 1.3.0\n" +
                       "".join("MM_AUDIT " + json.dumps(event) + "\n" for event in events))
        metadata = {
            "run_id": "test-run", "executed": True, "execution": "COMPLETE", "exit_status": 0,
            "shutdown_method": "quit", "truncated_streams": [], "capture_errors": [], "crash_dumps": [],
            "map_visits": [{"index": 0, "requested_map": "map_one", "after_sequence": suite_start,
                            "suite_sequence": first_suite},
                           {"index": 1, "requested_map": "map_two", "after_sequence": last,
                            "suite_sequence": second_suite}],
            "map_transitions": [{"index": 1, "requested_map": "map_two", "result": "PASS",
                                 "deactivated": True, "suite_completed": True,
                                 "callback_sequences": list(range(first, last + 1))}],
            "mapped_artifacts": {"engine": {"engine": True, "engine-module": True},
                                 "metamod": {"metamod": True}, "gamedll": {"game": True},
                                 "plugin": {"audit": True}},
        }
        return runner.aggregate(output, runtime, metadata, [log], manual)

    def evaluate(self, *runs, profile="core"):
        report = {"schema_version": 1, "run_id": "test-run", "runs": list(runs)}
        self.report.write_text(json.dumps(report), encoding="utf-8")
        return certification.evaluate(report, self.report, profile, runner.SCHEMA, runner.parse_event)

    def verdict(self, run, profile="core"):
        return self.evaluate(run, profile=profile)["runtimes"][0]["outcome"]

    def test_core_version_and_prebuilt_provenance(self):
        run = self.collect()
        result = self.evaluate(run)
        self.assertEqual(result["profile"], {
            "name": "core", "version": 2, "required_probes": sorted(certification.CORE)})
        row = result["runtimes"][0]
        self.assertEqual(row["outcome"], "CERTIFIED")
        self.assertEqual(row["runtime"]["plugin"]["artifact_manifest"]["sdk_commit"], "c" * 40)
        self.assertTrue(all(item["supported"] for item in row["required_probes"].values()))
        self.assertEqual(result["report"]["sha256"], hashlib.sha256(self.report.read_bytes()).hexdigest())

    def test_profiles_are_cumulative_and_windows_is_supported(self):
        core = self.collect("core")
        player = self.collect("player", "player", windows=True)
        extended = self.collect("extended", "extended")
        self.assertEqual(self.verdict(core, "player"), "INCOMPLETE")
        self.assertEqual(self.verdict(player, "player"), "CERTIFIED")
        self.assertEqual(self.verdict(player, "extended"), "INCOMPLETE")
        self.assertEqual(self.verdict(extended, "extended"), "CERTIFIED")

    def test_pass_observations_and_complete_protocol_do_not_fill_gaps(self):
        run = self.collect(omit={"trace.hull.v1", "cvar.roundtrip.v1"})
        self.assertTrue(run["protocol_complete"])
        self.assertEqual(run["exit_status"], 0)
        run["tests"] = [item for item in run["tests"] if item["result"] == "PASS"]
        self.assertEqual(self.verdict(run), "INCOMPLETE")

    def test_manual_and_unobserved_player_results_do_not_certify(self):
        test = "player.real_reuse.v1"
        run = self.collect(profile="player", omit={test},
                           manual=[{"test": test, "result": "PASS", "observed": "operator assertion"}])
        self.assertEqual(self.verdict(run, "player"), "INCOMPLETE")
        item = next(item for item in run["tests"] if item["test"] == test)
        item["source"] = "plugin"
        self.assertEqual(self.verdict(run, "player"), "INCOMPLETE")

    def test_failure_precedes_coverage_loss_and_unknown_provenance(self):
        run = self.collect(omit={"trace.line.v1"}, failures={"trace.hull.v1"})
        run["runtime"]["plugin"].pop("artifact_manifest")
        self.assertEqual(self.verdict(run), "FAILED")

    def test_identity_and_provenance(self):
        run = self.collect()
        manifest = run["runtime"]["plugin"].pop("artifact_manifest")
        self.assertEqual(self.verdict(run), "UNVERIFIED")
        run["runtime"]["plugin"]["artifact_manifest"] = manifest
        manifest["sha256"] = "a" * 64
        self.assertEqual(self.verdict(run), "FAILED")
        manifest["sha256"] = "d" * 64
        run["runtime"]["plugin"]["sdk_commit"] = "b" * 40
        self.assertEqual(self.verdict(run), "FAILED")
        del run["runtime"]["plugin"]["sdk_commit"]
        run["mapped_artifacts"]["gamedll"] = {}
        self.assertEqual(self.verdict(run), "UNVERIFIED")
        run["mapped_artifacts"]["gamedll"] = {"game": False}
        self.assertEqual(self.verdict(run), "FAILED")

    def test_runner_built_artifact_requires_clean_matching_checkout(self):
        run = self.collect()
        plugin = run["runtime"]["plugin"]
        plugin.update(audit_plugin_commit="a" * 40, sdk_commit="a" * 40, working_tree_dirty=False,
                      built_by_runner=True)
        del plugin["artifact_manifest"]
        self.assertEqual(self.verdict(run), "CERTIFIED")
        plugin["working_tree_dirty"] = True
        self.assertEqual(self.verdict(run), "UNVERIFIED")
        plugin["sdk_commit"] = "c" * 40
        self.assertEqual(self.verdict(run), "FAILED")

    def test_native_and_imported_evidence_ineligible(self):
        run = self.collect()
        run["execution"] = "IMPORTED"
        self.assertEqual(self.verdict(run), "UNVERIFIED")
        run["execution"] = "COMPLETE"
        run["evidence_scope"] = "native-fixture"
        self.assertEqual(self.verdict(run), "UNVERIFIED")

    def test_lifecycle_and_health(self):
        self.assertEqual(self.verdict(self.collect("no-drop", omit={"shutdown.drop.v1"})), "INCOMPLETE")
        run = self.collect("map")
        run["map_transitions"][0]["callback_sequences"].pop(1)
        self.assertEqual(self.verdict(run), "INCOMPLETE")
        run["capture_errors"] = ["read interrupted"]
        self.assertEqual(self.verdict(run), "INCOMPLETE")
        run["sanitizer_diagnostics"] = ["ERROR: AddressSanitizer"]
        self.assertEqual(self.verdict(run), "FAILED")

    def test_event_log_integrity(self):
        run = self.collect()
        (self.root / "live" / "events.jsonl").write_text("{}\n")
        self.assertEqual(self.verdict(run), "UNVERIFIED")

    def test_matching_hash_does_not_make_fixture_records_live(self):
        run = self.collect()
        path = self.root / "live" / "events.jsonl"
        data = path.read_bytes().replace(b'"evidence_scope": "runtime"', b'"evidence_scope": "native-fixture"')
        path.write_bytes(data)
        run["event_log_sha256"] = hashlib.sha256(data).hexdigest()
        self.assertEqual(self.verdict(run), "UNVERIFIED")

    def test_skip_and_inconclusive_are_coverage_gaps(self):
        run = self.collect(omit={"trace.hull.v1"})
        item = next(item for item in run["tests"] if item["test"] == "trace.hull.v1")
        for result in ("SKIP", "INCONCLUSIVE"):
            item["result"] = result
            self.assertEqual(self.verdict(run), "INCOMPLETE")

    def test_catalog_contracts_are_frozen(self):
        catalog = runner.catalog()
        for test, expected in certification.EXPECTATION_SHA256.items():
            self.assertEqual(hashlib.sha256(catalog[test].encode()).hexdigest(), expected, test)
        self.assertEqual(self.verdict(self.collect(altered="trace.line.v1")), "UNVERIFIED")

    def test_earlier_success_cannot_cover_a_later_map(self):
        run = self.collect(later_omit={"trace.line.v1"})
        self.assertTrue(run["protocol_complete"])
        self.assertEqual(self.verdict(run), "INCOMPLETE")

    def test_ubuntu_rerun_selection_without_provisioning(self):
        good = self.collect("good")
        failed = self.collect("failed", failures={"trace.line.v1"})
        gap = self.collect("gap", later_omit={"trace.line.v1"})
        unknown = self.collect("unknown")
        unknown["runtime"]["plugin"].pop("artifact_manifest")
        self.evaluate(good, failed, gap, unknown)
        checkout = Path(runner.__file__).parent
        script = (checkout / "ricochet_audit_ubuntu.sh").read_text()
        if os.name == "posix" and shutil.which("bash"):
            subprocess.run(["bash", "-n", str(checkout / "ricochet_audit_ubuntu.sh")], check=True)
        embedded = script.split("<<'PY_RERUN'\n", 1)[1].split("\nPY_RERUN", 1)[0]
        runs = self.root / "runs"
        source = runs / "01"
        report_dir = source / "report"
        report_dir.mkdir(parents=True)
        (report_dir / "report.json").write_bytes(self.report.read_bytes())
        rows = []
        for rid in ("good", "failed", "gap", "unknown"):
            shutil.copytree(self.root / rid, report_dir / rid)
            rows.append({"id": rid, "installation": str(self.root)})
        matrix = {"schema_version": 1, "runtimes": rows}
        for name, saved, report in [("01", matrix, None),
                                    ("02", {**matrix, "runtimes": []}, {"schema_version": 1, "runs": []}),
                                    ("03", matrix, {"schema_version": 1, "runs": [good]})]:
            folder = runs / name
            folder.mkdir(exist_ok=True)
            (folder / "matrix.json").write_text(json.dumps(saved))
            (folder / "sources.json").write_text(json.dumps({"repo_commit": "a" * 40}))
            if report is not None:
                (folder / "report").mkdir()
                (folder / "report/report.json").write_text(json.dumps(report))
        self.assertEqual(runner.latest_rerun_source(runs), source)
        for profile, expected in [("", {"failed"}), ("core", {"failed", "gap", "unknown"})]:
            dest = self.root / ("rerun-" + profile)
            dest.mkdir()
            selected = dest / "selected.txt"
            subprocess.run([sys.executable, "-c", embedded, str(source), str(dest),
                            "1", "0", "2", str(selected), profile, str(checkout)],
                           check=True, capture_output=True, text=True)
            self.assertEqual(set(selected.read_text().splitlines()), expected)

    def test_cli_mixed_matrix_schema_and_exit_codes(self):
        good = self.collect("good")
        bad = self.collect("bad", failures={"cvar.roundtrip.v1"})
        self.evaluate(good, bad)
        command = [sys.executable, runner.__file__, "certify", str(self.report), "core"]

        def invoke(name):
            return subprocess.run([*command, str(self.root / name)], capture_output=True, text=True)

        response = invoke("mixed")
        self.assertEqual(response.returncode, 1, response.stderr)
        result = runner.load(self.root / "mixed/certificate.json")
        self.assertEqual(result["schema_version"], 1)
        self.assertFalse(result["all_certified"])
        self.assertEqual(result["audit_protocol_version"], runner.SCHEMA)
        self.assertEqual([row["outcome"] for row in result["runtimes"]], ["CERTIFIED", "FAILED"])
        self.assertIn("bad — FAILED", (self.root / "mixed/certificate.md").read_text(encoding="utf-8"))
        self.evaluate(good)
        self.assertEqual(invoke("pass").returncode, 0)
        self.report.write_text('{"schema_version": 99, "runs": []}')
        self.assertEqual(invoke("invalid").returncode, 2)
        self.assertFalse((self.root / "invalid").exists())
