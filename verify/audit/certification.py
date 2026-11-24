"""Versioned compatibility policy for collected live audit reports."""

import hashlib
import json
import re

from evidence import identity as runtime_identity

CERTIFICATE_SCHEMA = 1
CORE = frozenset("""
    init.v1 commands.register.v1 command.arguments.v1 cvar.roundtrip.v1
    string.copy.v1 string.limit.v1 errors.v1 entity.discovery.v1
    entity.snapshot.v1 trace.line.v1 trace.hull.v1 map.activate.v1
    map.deactivate.v1 map.order.v1 map.stale.v1 shutdown.v1 shutdown.drop.v1
""".split())
PLAYER = CORE | frozenset("""
    player.join.v1 player.disconnect.v1 player.identity.v1 player.reuse.v1 player.real_reuse.v1
""".split())
PROFILES = {
    "core": CORE,
    "server-state": CORE | {"server.state.v1"},
    "player": PLAYER,
    "extended": PLAYER | {"entity.keyvalue_hook.v1", "lag.pre.v1", "lag.post.v1", "resources.repeat.v1"},
}
PROFILE_VERSIONS = {"core": 2, "player": 2, "extended": 2, "server-state": 1}
MAP_SUITE_PROBES = frozenset({"entity.discovery.v1", "entity.snapshot.v1", "trace.line.v1", "trace.hull.v1"})
# Freeze the catalog contracts without duplicating their descriptive text.
EXPECTATION_SHA256 = {
    "server.state.v1": "7e67299495b2f0e2f23e22b2692c048112f560e67fa843c2e4c153c0bc958b84",
    "command.arguments.v1": "1fb0a04e02e3f96631084125add940231cb91f201e79cce8f3ad4d53cac76046",
    "commands.register.v1": "cec78d7a41334cd8c0571dcfa5605dfe5397601be6d27425ea502f8edc92f295",
    "cvar.roundtrip.v1": "3a3f5c04502d6774befe6957373a4f6e7e8439c9f192940166bc095fab525a82",
    "entity.discovery.v1": "84634d04eda85c9f0d7546878daf1ed79fdc3780d5d8c28375872abe995ffa60",
    "entity.keyvalue_hook.v1": "2d7e795a9e733fc6f70794ff04189807faf15ab0929ec0f9618914b1431cf73a",
    "entity.snapshot.v1": "9b0c70fc0154129440a83487c49ddc7d43ea3978b6558d38096e03fc8e151e78",
    "errors.v1": "48ffe0a3bfc9c5155fe3b28f3c23dd1c6380d51b60bb4fe0c6c737324c0fd131",
    "init.v1": "6a7fa5e4f1a75402f63319728c65fa003df7e2615b4632d21303be86449d4169",
    "lag.post.v1": "4f2b6aa5b359ef9085926ad7c2a1e98f486a9d576b22b54777ee847f3cee9727",
    "lag.pre.v1": "596f2b088a448bdecf07ce552baa5ab419144ae4f874f02ce336f669f3662bb0",
    "map.activate.v1": "91045e5b70adaff3958100869b713ae6fb74bf5224dab020acf94109e58d1f08",
    "map.deactivate.v1": "bf05f26b7954d186e781cb2f4f72bfb04b596601c0ecf8d89459ed9a543e1f5d",
    "map.order.v1": "bf2c90a768bb92a69fdbdad24bf235480326e291ba039589693490ab3fb94c33",
    "map.stale.v1": "c9db00d755d87795334fa7e4535746ec1dab11ceb6ddeab50139dfa890e10f42",
    "player.disconnect.v1": "374a4ced19f331393c9fd051d57a041c1750738f00386d1cb67ebb5082c71f9f",
    "player.identity.v1": "59f399572ce63a2c2697c1e27f9a9b6f95fcc2ffb9bb5f894b20790077e7938c",
    "player.join.v1": "b79141a5b393abf4acb995899c8b037df1e5aa0f4f2d890a2b907f82b20e33b5",
    "player.real_reuse.v1": "c20a3f3efc999c6f8498a9410c503ec6a1b7c757478e4a411d8c0db246baad0b",
    "player.reuse.v1": "af224a66bb31001863870af33035d7fc641aac9cc873fe5ab14d29cd6025885c",
    "resources.repeat.v1": "6cd85ae244ea4b1b3e111ac6844ab4aab1f855d7b729382faff41450b16bddd6",
    "shutdown.drop.v1": "414cc1e80a7624af314c7221e8fe50bfa0c306d697e6b0e93b9c97d8117dd6db",
    "shutdown.v1": "d1fd069d81fbfc8b7458fe5ad49535091409a18de6ae5ba7ff108b122c5d70f1",
    "string.copy.v1": "9f48b278539c9dafddc70f6b1bfd563ef090a52c75c2bd9909202427f5fd45a9",
    "string.limit.v1": "ed1b26afefb8ffc477113aa4c27f219e69114f3b68f8f003f2a1c82b3f1fbc90",
    "trace.hull.v1": "2eb745e46590681237c71fca001d461aeeca3a87c5130daac8892c663cec9465",
    "trace.line.v1": "bbc995d4dfb39f870c3ad7696a78e20cd2baca095e0540294270e18c4b3220d5",
}


def canonical(event):
    return hashlib.sha256(event["expected"].encode()).hexdigest() == EXPECTATION_SHA256[event["test"]]
COMPONENTS = ("engine", "metamod", "gamedll", "plugin")
SHA256 = re.compile(r"[0-9a-f]{64}")
COMMIT = re.compile(r"[0-9a-f]{40}")


def matches(pattern, value):
    return isinstance(value, str) and pattern.fullmatch(value) is not None


def event_records(run, path, parse_event):
    with path.open("rb") as stream:
        data = stream.read(32 * 1024 * 1024 + 1)
    if len(data) > 32 * 1024 * 1024 or hashlib.sha256(data).hexdigest() != run.get("event_log_sha256"):
        raise ValueError("event log hash is missing or differs from the report")
    records = []
    for seq, line in enumerate(data.splitlines(), 1):
        record = json.loads(line)
        if (record.get("run_id") != run["run_id"] or record.get("runtime_id") != run["runtime"]["id"]
                or record.get("evidence_scope") != "runtime" or record.get("sequence_conflict") is not False
                or record.get("seq") != seq or parse_event(b"MM_AUDIT " + line + b"\n") is None):
            raise ValueError("invalid, out-of-order, or foreign event record")
        records.append(record)
    return records


def map_suite_probes(profile):
    return MAP_SUITE_PROBES | ({"server.state.v1"} if profile == "server-state" else set())


def check_lifecycle(run, runtime, records, incomplete, map_probes=MAP_SUITE_PROBES):
    ready = [e for e in records if e["kind"] == "ready"]
    statuses = [e["seq"] for e in records if e["kind"] == "status"]
    suites = [e["seq"] for e in records if e["kind"] == "suite_complete" and e["suite"] == "all"]
    reports = [e["seq"] for e in records if e["kind"] == "report"]
    shutdown, drop = (
        [e["seq"] for e in records if e.get("test") == test and e["result"] == "PASS"]
        for test in ("shutdown.v1", "shutdown.drop.v1")
    )
    if (not suites or len(ready) != 1 or ready[0]["submitted"] is not True
            or len(reports) != 1 or len(shutdown) != 1 or len(drop) != 1
            or not any(ready[0]["seq"] < status < suites[0] for status in statuses)
            or not suites[-1] < reports[0] < shutdown[0] < drop[0]):
        incomplete.append("Need ordered readiness, status, all-suite, report, GameShutdown and destructor records.")
    itinerary = runtime.get("map_itinerary", [])
    transitions = run.get("map_transitions", [])
    if len(set(itinerary)) < 2 or len(suites) != len(itinerary) or len(transitions) != len(itinerary) - 1:
        incomplete.append("Run the full itinerary with at least two distinct maps and one all-suite per visit.")
        return
    visits = run.get("map_visits", [])
    if len(visits) != len(itinerary):
        incomplete.append("Missing recorded all-suite windows for map visits.")
    for index, visit in enumerate(visits[:len(itinerary)]):
        start = visit.get("after_sequence")
        end = suites[index]
        lower = suites[index - 1] if index else (ready[0]["seq"] if len(ready) == 1 else 0)
        if (visit.get("index") != index or visit.get("requested_map") != itinerary[index]
                or visit.get("suite_sequence") != end or type(start) is not int or not lower <= start < end):
            incomplete.append(f"Map visit {index + 1} has an invalid suite window.")
            continue
        window = [e for e in records if start < e["seq"] < end]
        if any(e.get("test") in {"map.activate.v1", "map.deactivate.v1"} for e in window):
            incomplete.append(f"Map visit {index + 1} changed maps during its suite.")
        passed = {e["test"] for e in window if e.get("result") == "PASS"
                  and e.get("test") in map_probes and canonical(e)}
        if missing := map_probes - passed:
            incomplete.append(f"Map visit {index + 1} to {itinerary[index]} lacks PASS for {', '.join(sorted(missing))}.")
        activation_floor = lower if index else 0
        if not any(activation_floor < e["seq"] <= start and e.get("test") == "map.activate.v1"
                   and e["result"] == "PASS" and canonical(e) for e in records):
            incomplete.append(f"Map visit {index + 1} lacks activation before the suite command.")
    by_seq = {e["seq"]: e for e in records}
    for index, transition in enumerate(transitions, 1):
        seqs = transition.get("callback_sequences", [])
        relevant = [by_seq[seq] for seq in seqs if seq in by_seq and suites[index - 1] < seq < suites[index]]
        passed = {record.get("test") for record in relevant if record.get("result") == "PASS" and record.get("test") in EXPECTATION_SHA256 and canonical(record)}
        activations = [record["seq"] for record in relevant if record.get("test") == "map.activate.v1"
                       and record.get("result") == "PASS"]
        deactivations = [record["seq"] for record in relevant if record.get("test") == "map.deactivate.v1"
                         and record.get("result") == "PASS"]
        if (transition.get("index") != index or transition.get("requested_map") != itinerary[index]
                or transition.get("result") != "PASS" or transition.get("suite_completed") is not True
                or transition.get("deactivated") is not True or not relevant or len(relevant) != len(seqs)
                or not {"map.order.v1", "map.stale.v1"} <= passed
                or not any(before < after for before in deactivations for after in activations)):
            incomplete.append(f"Map transition {index} to {itinerary[index]} lacks ordered lifecycle or stale-handle evidence.")


def evaluate_run(run, report_path, profile, protocol, parse_event):
    runtime = run["runtime"]
    runtime_id = runtime["id"]
    failed, unverified, incomplete = [], [], []
    probes = {}
    records = []
    event_path = report_path.parent / runtime_id / "events.jsonl"
    if run.get("evidence_scope") != "runtime" or run.get("execution") == "IMPORTED" or run.get("native_fixture_results"):
        unverified.append("Certification requires an automated live-server report. Imports and native fixtures are ineligible.")
    else:
        if type(run.get("schema_version")) is not int or run["schema_version"] != protocol:
            unverified.append("The runtime row is missing the supported audit report schema.")
        try:
            records = event_records(run, event_path, parse_event)
        except (OSError, ValueError, KeyError, TypeError, AttributeError) as error:
            unverified.append(f"Cannot corroborate plugin observations from events.jsonl: {error}")

        for field in ("diagnostics", "sanitizer_diagnostics", "crash_diagnostics", "crash_dumps"):
            if run.get(field):
                failed.append(f"Observed {field.replace('_', ' ')}. Inspect the runtime logs.")
        if type(run.get("exit_status")) is int and run["exit_status"] != 0:
            failed.append(f"Server exited with status {run['exit_status']}.")
        if run.get("executed") is not True or run.get("execution") != "COMPLETE":
            incomplete.append("Live execution did not complete. Inspect the runtime error and rerun.")
        if (type(run.get("exit_status")) is not int or run["exit_status"] != 0
                or run.get("shutdown_method") != "quit"):
            incomplete.append("Need a zero exit status after a console quit.")
        if (run.get("protocol_complete") is not True
                or any(run.get(field) != [] for field in ("protocol_warnings", "capture_errors", "truncated_streams"))
                or any(record["kind"] == "limit" for record in records)
                or run.get("error")):
            incomplete.append("Need a complete protocol and intact console capture.")
        observed_identity = runtime_identity(runtime, run.get("console_version_observations", []),
                                             records, run.get("mapped_artifacts", {}))
        for component in COMPONENTS:
            identity = run.get("runtime_identity", {}).get(component, {})
            observed = observed_identity[component]
            if identity.get("result") == "FAIL" or observed["result"] == "FAIL":
                failed.append(f"{component} identity failed: {observed['reasons'] or identity.get('reasons', [])}.")
            elif identity.get("result") != "PASS" or observed["result"] != "PASS":
                unverified.append(f"Verify {component} identity with the runtime identity checks.")
        if run.get("identity_failures"):
            failed.append(f"Identity mismatches: {run['identity_failures']}.")
        if any(t.get("result") == "FAIL" for t in run.get("map_transitions", [])):
            failed.append("An observed map transition failed.")

        results = run.get("tests", [])
        summaries = {item["test"]: item for item in results}
        if len(summaries) != len(results):
            unverified.append("Duplicate probe summaries.")
        for item in results:
            if item.get("source") == "plugin" and item.get("result") == "FAIL":
                failed.append(f"{item['test']} failed: {item.get('observed', '')}")
            if item.get("manual_conflict"):
                unverified.append(f"Conflicting manual evidence for {item['test']}.")
        events_by_test = {}
        for record in records:
            if record["kind"] == "result":
                events_by_test.setdefault(record["test"], []).append(record)
        for test, events in events_by_test.items():
            if any(event["result"] == "FAIL" for event in events):
                failed.append(f"Plugin event records an observed failure for {test}.")
        for test in sorted(PROFILES[profile]):
            result = summaries.get(test, {})
            observations = events_by_test.get(test, [])
            if any(not canonical(event) for event in observations):
                unverified.append(f"{test}: expectation differs from the versioned probe contract.")
            passes = [e for e in observations if e["result"] == "PASS" and canonical(e)]
            matched = [e for e in passes if e["seq"] == result.get("seq")
                       and e["seq"] in result.get("event_sequences", [])
                       and e["observed"] == result.get("observed") and e["expected"] == result.get("expected")]
            supported = result.get("source") == "plugin" and result.get("result") == "PASS" and bool(matched)
            probes[test] = {"result": result.get("result", "MISSING"), "source": result.get("source"),
                            "supported": supported, "event_sequences": [e["seq"] for e in matched],
                            "observed": result.get("observed")}
            if not supported and result.get("result") != "FAIL":
                incomplete.append(f"{test}: need a corroborated plugin PASS (found {probes[test]['result']}).")
            elif not supported and result.get("source") != "plugin":
                unverified.append(f"{test}: manual failure is not a live probe observation.")
        check_lifecycle(run, runtime, records, incomplete, map_suite_probes(profile))

        components = runtime.get("components", {})
        runner_revision = runtime.get("runner_revision", {})
        if (not matches(COMMIT, runner_revision.get("commit"))
                or runner_revision.get("working_tree_dirty") is not False):
            unverified.append("Collect the report from a clean, identified audit runner checkout.")
        for name in COMPONENTS[:-1]:
            item = components.get(name, {})
            if (not all(isinstance(item.get(field), str) and item[field].strip()
                        for field in ("implementation", "version", "binary"))
                    or not matches(SHA256, item.get("sha256"))):
                unverified.append(f"Need exact {name} binary, implementation, version and SHA-256.")
        engine = components.get("engine", {})
        if any(not matches(SHA256, value) for value in engine.get("modules", {}).values()):
            unverified.append("Need SHA-256 for each separately loaded engine module.")
        plugin = runtime.get("plugin", {})
        manifest = plugin.get("artifact_manifest", {})
        if not matches(SHA256, plugin.get("sha256")):
            unverified.append("Need the deployed audit plugin SHA-256.")
        if plugin.get("built_by_runner") is True:
            provenance = plugin
            if plugin.get("working_tree_dirty") is not False:
                unverified.append("Rebuild the audit plugin from a clean source checkout.")
            if any(plugin.get(key) != runner_revision.get("commit") for key in ("audit_plugin_commit", "sdk_commit")):
                failed.append("Runner-built plugin revisions differ from the captured build checkout.")
        else:
            provenance = manifest
            if type(manifest.get("schema_version")) is not int or manifest["schema_version"] != 1:
                unverified.append("Supply a version 1 build provenance manifest for the prebuilt plugin.")
            if manifest.get("working_tree_dirty") is not False:
                unverified.append("The build provenance must identify a clean artifact source checkout.")
            if manifest and manifest.get("sha256") != plugin.get("sha256"):
                failed.append("Build provenance SHA-256 differs from the deployed plugin.")
            if any(plugin.get(key) is not None and plugin[key] != manifest.get(key)
                   for key in ("audit_plugin_commit", "sdk_commit")):
                failed.append("Configured revisions disagree with the artifact provenance manifest.")
        if any(not matches(COMMIT, provenance.get(key)) for key in ("audit_plugin_commit", "sdk_commit")):
            unverified.append("Record full audit plugin and SDK commit IDs from the artifact build.")
        host = runtime.get("host", {})
        expected_target = {"linux": ("i686", "i686-unknown-linux-gnu"), "windows": ("x86", "i686-pc-windows-msvc")}
        if (expected_target.get(runtime.get("os")) != (runtime.get("architecture"), plugin.get("target"))
                or any(not isinstance(host.get(field), str) or not host[field].strip()
                       for field in ("system", "machine", "release", "version"))
                or host.get("system", "").lower() != runtime.get("os")):
            unverified.append("Need consistent OS, host release, architecture and plugin target.")
    outcome = ("FAILED" if failed else "UNVERIFIED" if unverified else
               "INCOMPLETE" if incomplete else "CERTIFIED")
    return {"runtime_id": runtime_id, "outcome": outcome, "runtime": runtime, "required_probes": probes,
            "map_visits": run.get("map_visits", []),
            "identity": run.get("runtime_identity", {}), "map_transitions": run.get("map_transitions", []),
            "execution": {key: run.get(key) for key in ("execution", "exit_status", "shutdown_method",
                                                        "protocol_complete")},
            "evidence": {"event_log": str(event_path), "event_log_sha256": run.get("event_log_sha256"),
                         "run_id": run.get("run_id")},
            "reasons": {"failed": failed, "unverified": unverified, "incomplete": incomplete}}


def evaluate(report, path, profile, protocol, parse_event):
    runs = report["runs"]
    if (type(report.get("schema_version")) is not int or report["schema_version"] != protocol
            or not isinstance(runs, list) or not runs or not isinstance(report.get("run_id"), str)
            or not report["run_id"].strip()):
        raise ValueError("expected a nonempty audit report with a supported schema")
    ids = [run["runtime"]["id"] for run in runs]
    if (any(not isinstance(value, str) or not re.fullmatch(r"[A-Za-z0-9_-]{1,64}", value) for value in ids)
            or len(ids) != len(set(ids)) or any(run.get("run_id") != report["run_id"] for run in runs)):
        raise ValueError("invalid runtime IDs or inconsistent report run ID")
    rows = [evaluate_run(run, path.resolve(), profile, protocol, parse_event) for run in runs]
    return {"schema_version": CERTIFICATE_SCHEMA, "audit_protocol_version": protocol,
            "all_certified": all(row["outcome"] == "CERTIFIED" for row in rows),
            "profile": {"name": profile, "version": PROFILE_VERSIONS[profile],
                        "required_probes": sorted(PROFILES[profile])},
            "report": {"path": str(path.resolve()), "sha256": hashlib.sha256(path.read_bytes()).hexdigest()},
            "runtimes": rows}


def markdown(certificate):
    profile = certificate["profile"]
    lines = ["# Metamod SDK for Rust compatibility evaluation", "",
             f"Profile: **{profile['name']} v{profile['version']}**", "",
             "Each verdict applies to the recorded binaries and tested itinerary. "
             "It is neither universal compatibility nor a memory-safety guarantee.", ""]
    for row in certificate["runtimes"]:
        runtime = row["runtime"]
        lines += [f"## {row['runtime_id']} — {row['outcome']}", ""]
        if runtime.get("os") and runtime.get("architecture"):
            lines.append(f"- Platform: `{runtime['os']} / {runtime['architecture']}`")
        for name, component in runtime.get("components", {}).items():
            if name == "engine":
                # The executable can be identical across engines with different modules.
                modules = component.get("modules", {})
                if component.get("sha256") or modules:
                    lines.append(f"- Engine: {component.get('implementation')} {component.get('version')}")
                if component.get("sha256"):
                    binary = component.get("binary")
                    label = "Launcher" if modules else "Binary"
                    filename = f" `{binary}`" if binary else ""
                    lines.append(f"  - {label}{filename} SHA-256: `{component['sha256']}`")
                for module, sha256 in sorted(modules.items()):
                    lines.append(f"  - Engine module `{module}` SHA-256: `{sha256}`")
                continue
            if component.get("sha256"):
                lines.append(f"- {name}: {component.get('implementation')} {component.get('version')}, "
                             f"SHA-256 `{component['sha256']}`")
        plugin = runtime.get("plugin", {})
        if plugin.get("sha256"):
            lines.append(f"- Audit plugin SHA-256: `{plugin['sha256']}`")
            revision = plugin if plugin.get("built_by_runner") else plugin.get("artifact_manifest", {})
            lines.extend(f"- {key}: `{revision[key]}`" for key in ("sdk_commit", "audit_plugin_commit")
                         if revision.get(key))
        for category, reasons in row["reasons"].items():
            for reason in reasons:
                lines.append(f"- **{category.title()}** {reason}".replace("\n", " "))
        if row["outcome"] == "CERTIFIED":
            lines.append(f"All {len(row['required_probes'])} mandatory probes were corroborated.")
        lines.append("")
    return "\n".join(lines)
