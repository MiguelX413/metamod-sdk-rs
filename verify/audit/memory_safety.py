"""Versioned memory-safety evidence, independent of compatibility policy."""

import hashlib
import os
import re
import shutil
import subprocess
import xml.etree.ElementTree as ET

VERSION = 1
MAX_XML = 32 * 1024 * 1024


def memcheck_command(binary, arguments, output):
    tool = shutil.which("valgrind")
    if tool is None:
        raise FileNotFoundError("Valgrind is unavailable (install it on the audit host)")
    if "%" in str(output):
        raise ValueError("Memcheck output path cannot contain a Valgrind filename expansion (%)")
    version = subprocess.check_output([tool, "--command-line-only=yes", "--version"],
                                      text=True, timeout=10).strip()
    if not version.startswith("valgrind-"):
        raise ValueError("could not identify the Valgrind version")
    command = [tool, "--command-line-only=yes", "--tool=memcheck", "--xml=yes", f"--xml-file={output}",
               "--trace-children=no", "--child-silent-after-fork=yes", "--leak-check=full",
               "--show-leak-kinds=definite,indirect,possible",
               "--errors-for-leak-kinds=definite,indirect,possible",
               "--track-origins=yes", "--num-callers=32", "--error-limit=no",
               binary, *arguments]
    return command, version


def memcheck_result(path, version, pid, executable, complete):
    result = {"status": "INCONCLUSIVE", "tool": "Valgrind Memcheck",
              "version": version, "diagnostics": path.name, "findings": [],
              "unresolved_findings": False}
    try:
        with path.open("rb") as stream:
            data = stream.read(MAX_XML + 1)
        if len(data) > MAX_XML:
            result["reason"] = "Memcheck XML exceeded the analysis limit; inspect the preserved file"
            return result
        result["diagnostics_sha256"] = hashlib.sha256(data).hexdigest()
        # Valgrind writes plain XML. Disallow DTDs to avoid entity expansion.
        if b"\x00" in data or b"<!DOCTYPE" in data or b"<!ENTITY" in data:
            raise ValueError("unsupported XML encoding or declaration")
        root = ET.fromstring(data)
        if (root.tag != "valgrindoutput" or root.findtext("protocolversion") not in {"4", "5", "6"}
                or root.findtext("protocoltool") != "memcheck"
                or root.findtext("pid") != str(pid)
                or root.findtext("args/argv/exe") != executable):
            raise ValueError("Memcheck XML does not match the launched process or supported protocol")
        for error in root.findall("error"):
            result["findings"].append({
                "id": error.findtext("unique"),
                "kind": error.findtext("kind"),
                "description": error.findtext("what") or error.findtext("xwhat/text"),
                "attribution": "UNRESOLVED",
            })
        result["suppression_counts"] = [
            {"name": pair.findtext("name"), "count": pair.findtext("count")}
            for pair in root.findall("suppcounts/pair")
        ]
        if result["findings"] or root.findall("errorcounts/pair"):
            result.update(status="FAIL", unresolved_findings=True)
        elif (complete and root.findall("status") and root.findall("status")[-1].findtext("state") == "FINISHED"
              and root.find("errorcounts") is not None and not root.findall("errorcounts/pair")
              and not root.findall("fatal_signal")):
            result["status"] = "PASS"
        else:
            result["reason"] = "missing clean Memcheck completion or incomplete runtime capture"
    except (OSError, ET.ParseError, ValueError) as error:
        result["reason"] = str(error)
    return result


def live_report(runs, source_report):
    rows = []
    for run in runs:
        runtime = run["runtime"]
        instrumentation = run["memory_instrumentation"]
        rows.append({
            "runtime_id": runtime["id"],
            "artifacts": {**runtime["components"], "plugin": runtime.get("plugin")},
            "host": runtime["host"],
            "architecture": runtime["architecture"],
            "runner_revision": runtime["runner_revision"],
            "compatibility": {"status": "NOT_RUN", "reason": "evaluate separately with runner.py certify"},
            "rust_sdk_soundness": {"status": "NOT_RUN", "reason": "live instrumentation cannot evaluate Rust soundness"},
            "native_runtime": {
                **instrumentation,
                "scenarios": {"map_visits": run.get("map_visits", []),
                              "shutdown": run.get("shutdown_method"),
                              "execution": run["execution"],
                              "exit_status": run["exit_status"],
                              "event_log_sha256": run["event_log_sha256"]},
                "unverified_assumptions": [
                    "unexercised native paths, Rust aliasing, and host contracts",
                    "tool default suppressions and potential instrumentation blind spots",
                ],
            },
        })
    return {"profile": {"name": "memory-safety", "version": VERSION},
            "scope": "live-runtime",
            "report": {"path": source_report.name, "sha256": hashlib.sha256(source_report.read_bytes()).hexdigest()},
            "runtimes": rows}


def markdown(report):
    lines = ["# Memory-safety evidence v1", "",
             "Each status describes its own evidence. A clean run covers only exercised paths.", "",
             "| Runtime | Compatibility | Rust SDK soundness | Native runtime |",
             "| --- | --- | --- | --- |"]
    for row in report["runtimes"]:
        lines.append("| " + " | ".join([row["runtime_id"], row["compatibility"]["status"],
                                       row["rust_sdk_soundness"]["status"], row["native_runtime"]["status"]]) + " |")
    for row in report["runtimes"]:
        native = row["native_runtime"]
        lines += ["", f"## {row['runtime_id']}", "",
                  f"- Tool version: {native.get('version', 'unavailable')}",
                  f"- Findings: {len(native.get('findings', []))}"]
        if native.get("diagnostics"):
            lines.append(f"- Diagnostics: `{row['runtime_id']}/{native['diagnostics']}`")
        if native.get("reason"):
            lines.append(f"- Qualification: {native['reason']}")
        if native.get("unresolved_findings"):
            lines.append("- Attribution requires review of the preserved native stacks.")
    return "\n".join(lines) + "\n"


MIRI_SCENARIOS = (
    "__ffi::tests::entity_appearance_owns_fields_and_rejects_interrupted_native_strings",
    "__ffi::tests::precaching_is_scoped_retains_static_names_and_invalidates_map_indices",
    "__ffi::tests::precache_panics_are_contained_and_scope_is_closed",
    "__ffi::tests::entity_relationships_observe_slots_without_reading_foreign_targets",
    "__ffi::tests::server_state_copies_globals_and_rejects_interrupted_strings",
    "hooks::tests::borrowed_reasons_keep_the_source_pointer",
    "__ffi::tests::borrowed_admission_is_snapshotted_before_deferred_mutation_and_shutdown",
    "__ffi::tests::borrowed_admission_is_discarded_after_deferred_panic",
    "__ffi::tests::admission_can_borrow_the_owned_connection_snapshot",
    "bounded_cstr::tests::borrowed_and_owned_preserve_bounds_and_allocations",
    "bounded_cstr::tests::boxed_traits_keep_c_string_equality_and_hashing",
    "bounded_cstr::tests::owned_strings_round_trip_through_boxed_views_without_copying",
    "bounded_cstr::tests::explicit_truncation_counts_bytes_and_keeps_a_terminator",
    "bounded_cstr::tests::truncation_checks_capacity_before_writing",
    "runtime::tests::owned_metamod_messages_validate_and_reuse_allocations",
    "hooks::tests::rejection_reasons_require_explicit_truncation",
    "message::tests::send_guard_releases_after_rust_unwind",
    "__ffi::tests::message_send_preserves_bytes_and_rejects_nested_and_interrupted_sends",
    "__ffi::tests::client_admission_is_synchronous_and_bounds_the_rejection_buffer",
    "__ffi::tests::client_admission_discards_rejection_after_nested_shutdown",
    "__ffi::tests::native_print_reentry_keeps_the_plugin_and_borrowed_buffer_live",
    "__ffi::tests::cvar_getter_discards_a_result_invalidated_during_native_reentry",
    "__ffi::tests::nested_lifecycle_is_deferred_and_panic_disables_user_hooks",
    "__ffi::tests::session_ids_expire_on_disconnect_slot_reuse_and_map_boundaries",
    "__ffi::tests::nested_shutdown_revokes_engine_access_and_cleans_up_once",
    "__ffi::tests::synchronous_results_survive_reentry_and_panics_fall_back",
    "engine::tests::cvar_records_are_distinct_stable_and_never_reused",
    "trace::tests::discovery_discards_retired_arena_pointer_after_lookup",
)


def miri_evidence(repo, output):
    """Run controlled Rust mocks and preserve exact tool output and inputs."""
    output = output.resolve()
    output.mkdir(parents=True)
    artifacts = sorted((repo / "metamod-sdk-rs" / "src").rglob("*.rs"))
    artifacts.extend(repo / name for name in ("Cargo.lock", "metamod-sys/build.rs", "metamod-sys/wrapper.hpp",
                                              "metamod-sys/src/lib.rs"))
    record = {
        "tool": "Miri",
        "artifacts": {path.relative_to(repo).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
                      for path in artifacts},
        "scenarios": [],
    }
    # In particular, do not inherit a request to disable leak or alias checks.
    env = os.environ.copy()
    env.pop("MIRIFLAGS", None)
    try:
        version = subprocess.run(["cargo", "+nightly", "miri", "--version"], cwd=repo,
                                 stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=False, timeout=60, env=env)
        (output / "miri-version.log").write_bytes(version.stdout)
        if version.returncode:
            record.update(status="NOT_RUN", reason="Miri version probe failed; see miri-version.log")
            return record
        record["version"] = version.stdout.decode(errors="replace").strip()
    except (OSError, subprocess.TimeoutExpired) as error:
        record.update(status="NOT_RUN", reason=str(error))
        return record

    for index, scenario in enumerate(MIRI_SCENARIOS, 1):
        args = ["cargo", "+nightly", "miri", "test", "--locked", "-p", "metamod-sdk-rs",
                "--lib", "--", "--exact", scenario]
        path = output / f"miri-{index}.log"
        status = "INCONCLUSIVE"
        with path.open("wb") as log:
            try:
                process = subprocess.run(args, cwd=repo, stdout=log, stderr=subprocess.STDOUT,
                                         timeout=600, check=False, env=env)
                transcript = path.read_text(encoding="utf-8", errors="replace")
                if process.returncode == 0 and re.search(r"test result: ok\. 1 passed;", transcript):
                    status = "PASS"
                elif process.returncode and ("Undefined Behavior" in transcript or
                                              "memory leaked" in transcript or
                                              "test result: FAILED" in transcript):
                    status = "FAIL"
            except (OSError, subprocess.TimeoutExpired) as error:
                log.write(f"\n{error}\n".encode())
        record["scenarios"].append({"name": scenario, "status": status, "argv": args,
                                    "log": path.name, "sha256": hashlib.sha256(path.read_bytes()).hexdigest()})

    statuses = {scenario["status"] for scenario in record["scenarios"]}
    record["status"] = ("FAIL" if "FAIL" in statuses else "PASS" if statuses == {"PASS"} else "INCONCLUSIVE")
    return record


def main():
    import argparse
    import json
    from pathlib import Path

    parser = argparse.ArgumentParser(description="Collect memory-safety v1 evidence for Rust-side Miri mocks")
    parser.add_argument("output", type=Path, help="new evidence directory")
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[2]
    try:
        evidence = miri_evidence(repo, args.output)
        report = {"profile": {"name": "memory-safety", "version": VERSION},
                  "scope": "rust-mocks",
                  "compatibility": {"status": "NOT_RUN"},
                  "rust_sdk_soundness": {
                      "status": "INCONCLUSIVE",
                      "reason": "mock test results alone do not resolve SDK soundness obligations",
                      "instrumentation": evidence,
                      "unverified_assumptions": [
                          "native callback contracts and pointer lifetime obligations",
                          "aliasing and reentry paths outside the selected mocks",
                      ],
                  },
                  "native_runtime": {"status": "NOT_RUN"}}
        (args.output / "memory-safety.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    except OSError as error:
        parser.exit(2, f"memory-safety: {error}\n")
    print(f"Miri scenarios: {evidence['status']}. Evidence: {args.output / 'memory-safety.json'}")
    return int(evidence["status"] != "PASS")


if __name__ == "__main__":
    raise SystemExit(main())
