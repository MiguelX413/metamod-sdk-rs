import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import memory_safety


class MemorySafetyTests(unittest.TestCase):
    def setUp(self):
        work = tempfile.TemporaryDirectory()
        self.addCleanup(work.cleanup)
        self.root = Path(work.name)
        self.xml = self.root / "memcheck.xml"

    def report(self, body, *, complete=True, pid=123, protocol="4"):
        self.xml.write_text(f"""<valgrindoutput>
            <protocolversion>{protocol}</protocolversion><protocoltool>memcheck</protocoltool><pid>123</pid>
            <args><argv><exe>/staged/hlds_linux</exe></argv></args>
            {body}
            </valgrindoutput>""", encoding="utf-8")
        return memory_safety.memcheck_result(self.xml, "valgrind-test", pid, "/staged/hlds_linux", complete)

    def test_pass_requires_completed_capture_and_matching_instrumentation(self):
        body = "<status><state>FINISHED</state></status><errorcounts/>"
        for protocol in ("4", "5", "6"):
            self.assertEqual(self.report(body, protocol=protocol)["status"], "PASS")
        self.assertEqual(self.report(body, complete=False)["status"], "INCONCLUSIVE")
        self.assertEqual(self.report(body, pid=456)["status"], "INCONCLUSIVE")
        self.assertEqual(self.report(body, protocol="3")["status"], "INCONCLUSIVE")
        self.assertEqual(self.report("<status><state>RUNNING</state></status>")["status"], "INCONCLUSIVE")
        self.assertEqual(self.report(body + "<fatal_signal/>")["status"], "INCONCLUSIVE")

    def test_findings_fail_even_with_incomplete_runtime_and_preserve_attribution(self):
        body = "<error><unique>0x1</unique><kind>InvalidRead</kind><what>Invalid read</what></error>"
        result = self.report(body, complete=False)
        self.assertEqual(result["status"], "FAIL")
        self.assertTrue(result["unresolved_findings"])
        self.assertEqual(result["findings"][0]["attribution"], "UNRESOLVED")
        self.assertTrue(result["diagnostics_sha256"])
        self.assertEqual(self.report("<errorcounts><pair/></errorcounts>")["status"], "FAIL")

    def test_missing_or_truncated_diagnostics_are_inconclusive(self):
        for content in (None, "<valgrindoutput>", "<!DOCTYPE valgrindoutput><valgrindoutput/>"):
            if content is not None:
                self.xml.write_text(content, encoding="utf-8")
            result = memory_safety.memcheck_result(self.xml, "valgrind-test", 123, "/staged/hlds_linux", True)
            self.assertEqual(result["status"], "INCONCLUSIVE")

    def test_cli_reports_unsupported_without_certifying_other_dimensions(self):
        config = self.root / "matrix.json"
        config.write_text(json.dumps({"schema_version": 1, "runtimes": [
            {"id": "unsupported", "os": "windows", "installation": "missing"}
        ]}), encoding="utf-8")
        output = self.root / "output"
        runner = Path(__file__).with_name("runner.py")
        result = subprocess.run([sys.executable, str(runner), "run", str(config), str(output), "--memcheck"],
                                capture_output=True, text=True, check=False)
        self.assertEqual(result.returncode, 1, result.stderr)
        report = json.loads((output / "memory-safety.json").read_text(encoding="utf-8"))
        self.assertEqual(report["profile"], {"name": "memory-safety", "version": 1})
        row, = report["runtimes"]
        self.assertEqual(row["native_runtime"]["status"], "UNSUPPORTED")
        self.assertEqual(row["compatibility"]["status"], "NOT_RUN")
        self.assertEqual(row["rust_sdk_soundness"]["status"], "NOT_RUN")

    def test_miri_artifacts_include_nested_rust_modules_and_tests(self):
        sources = {
            "metamod-sdk-rs/src/lib.rs": "mod __ffi;",
            "metamod-sdk-rs/src/__ffi/mod.rs": "mod state;",
            "metamod-sdk-rs/src/__ffi/state.rs": "struct State;",
            "metamod-sdk-rs/src/__ffi/tests/lifecycle.rs": "fn lifecycle() {}",
            "Cargo.lock": "lock",
            "metamod-sys/build.rs": "fn main() {}",
            "metamod-sys/wrapper.hpp": "// headers",
            "metamod-sys/src/lib.rs": "// bindings",
        }
        for name, content in sources.items():
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content, encoding="utf-8")
        unavailable = subprocess.CompletedProcess([], 1, stdout=b"Miri unavailable")
        with patch.object(memory_safety.subprocess, "run", return_value=unavailable):
            record = memory_safety.miri_evidence(self.root, self.root / "miri-output")
        self.assertEqual(record["status"], "NOT_RUN")
        self.assertEqual(record["artifacts"], {
            name: hashlib.sha256(content.encode()).hexdigest()
            for name, content in sources.items()
        })
