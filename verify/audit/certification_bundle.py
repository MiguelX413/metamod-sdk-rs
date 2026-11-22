"""Portable, deliberately limited input to the existing certification policy."""

import json
import os
from pathlib import Path
import re
import shutil
import tempfile

from artifacts import digest
import certification
from evidence import BUILD, ENGINE, GAME_PATH, MANAGER, PLUGIN_ROW

SCHEMA = 1
FILES = {"report.json", "matrix.json", "certificate.json", "certificate.md"}
COMPONENT_FIELDS = ("implementation", "version", "binary", "console_version", "console_version_field")
RUN_FIELDS = (
    "schema_version", "run_id", "evidence_scope", "execution", "executed", "exit_status", "shutdown_method",
    "protocol_complete", "protocol_warnings", "capture_errors", "truncated_streams", "diagnostics",
    "sanitizer_diagnostics", "crash_diagnostics", "crash_dumps", "identity_failures", "event_log_sha256",
    "map_visits", "map_transitions",
)
PRIVATE = re.compile(
    r"(?:[A-Za-z]:[\\/]|\\\\[^?]|(?<![\w/])/[A-Za-z0-9_.~-]+/"
    r"|\b(?:password|passwd|access[_ -]?token|api[_ -]?key|authorization)\s*[:=]"
    r"|\bSTEAM_[0-9]+:[0-9]+:[0-9]+\b|[\w.+-]+@[\w.-]+\.[a-z]{2,})", re.I
)
RELATIVE = re.compile(r"[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.-]+)*")
EVENT_FIELDS = {"schema_version", "seq", "kind", "run_id", "runtime_id", "evidence_scope", "sequence_conflict"}
EVENT_PAYLOADS = {
    "result": {"test", "result", "expected", "observed"},
    "ready": {"submitted"},
    "status": {"callback"},
    "report": {"complete"},
    "suite_complete": {"suite"},
    "usage": {"suites"},
    "limit": set(),
}


def pick(value, fields):
    return {key: value[key] for key in fields if key in value}


def write(path, value):
    path.write_text(json.dumps(value, sort_keys=True, indent=2, ensure_ascii=True) + "\n", encoding="utf-8")


def safe_text(value):
    if isinstance(value, str):
        if PRIVATE.search(value):
            raise ValueError("potentially private data in evidence; review locally before publishing")
    elif isinstance(value, dict):
        for key, item in value.items():
            safe_text(key)
            safe_text(item)
    elif isinstance(value, (list, tuple)):
        for item in value:
            safe_text(item)


def local_name(path):
    path = path.replace("\\", "/")
    if not RELATIVE.fullmatch(path) or set(path.split("/")) & {".", ".."}:
        raise ValueError("expected a public relative artifact name")
    return path


def public_path(path, root, windows):
    # Map only paths belonging to this run's staging root. Keep relative
    # observations relative, as the identity evaluator distinguishes them.
    path = path.replace("\\", "/").removeprefix("//?/")
    old = root.replace("\\", "/").removeprefix("//?/").rstrip("/")
    compared, prefix = (path.casefold(), old.casefold()) if windows else (path, old)
    if compared.startswith(prefix + "/"):
        relative = local_name(path[len(old) + 1:])
        if windows:
            return "X:\\audit-stage\\" + relative.replace("/", "\\")
        return "/audit-stage/" + relative
    return local_name(path)


def console_lines(lines, root, windows):
    result = []
    for line in lines:
        if match := ENGINE.search(line):
            name, version, variant = match.groups()
            result.append(f"{name}: {version}{variant or ''}")
        if match := BUILD.search(line):
            result.append(f"Exe build: ({match[1]})")
        if match := MANAGER.search(line):
            result.append(f"{match[1]} version: {match[2]}")
        if match := GAME_PATH.search(line):
            result.append(f"GameDLL pathname: {public_path(match[1].strip(' \"\''), root, windows)}")
        if match := PLUGIN_ROW.search(line):
            observation = match[0].strip()
            safe_text(observation)
            result.append(observation)
    return result


def public_run(run):
    row = pick(run, RUN_FIELDS)
    runtime = run["runtime"]
    public = pick(runtime, ("id", "os", "architecture", "map_itinerary", "experimental"))
    public["runner_revision"] = pick(runtime["runner_revision"], ("commit", "working_tree_dirty"))
    public["host"] = pick(runtime["host"], ("system", "machine", "release", "version"))
    if "distribution" in runtime["host"]:
        public["host"]["distribution"] = pick(runtime["host"]["distribution"], ("ID", "VERSION_ID"))
    public["components"] = {
        name: pick(value, (*COMPONENT_FIELDS, "loaded_binary", "sha256", "modules"))
        for name, value in runtime["components"].items()
    }
    plugin = runtime["plugin"]
    public["plugin"] = pick(plugin, ("implementation", "binary", "target", "sha256", "built_by_runner",
                                    "working_tree_dirty", "sdk_commit", "audit_plugin_commit"))
    if "artifact_manifest" in plugin:
        public["plugin"]["artifact_manifest"] = pick(
            plugin["artifact_manifest"], ("schema_version", "sha256", "working_tree_dirty",
                                          "sdk_commit", "audit_plugin_commit"))
    for value in [*public["components"].values(), public["plugin"]]:
        for key in ("binary", "loaded_binary"):
            if key in value:
                local_name(value[key])
        for module in value.get("modules", {}):
            local_name(module)
    row["tests"] = [pick(test, ("test", "result", "source", "seq", "event_sequences", "expected",
                                "observed", "manual_conflict"))
                    for test in run["tests"] if test.get("source") == "plugin"]
    row["map_visits"] = [pick(visit, ("index", "requested_map", "after_sequence", "suite_sequence"))
                         for visit in run["map_visits"]]
    row["map_transitions"] = [
        pick(transition, ("index", "requested_map", "result", "deactivated", "suite_completed",
                          "callback_sequences", "previous_handle_checks"))
        for transition in run["map_transitions"]
    ]
    safe_text({**row, "runtime": public})
    windows = public["os"] == "windows"
    old = runtime["staged_root"]
    public["staged_root"] = "X:\\audit-stage" if windows else "/audit-stage"
    row["runtime"] = public
    row["console_version_observations"] = console_lines(run["console_version_observations"], old, windows)
    row["mapped_artifacts"] = {
        name: {public_path(path, old, windows): found for path, found in mappings.items()}
        for name, mappings in run["mapped_artifacts"].items()
    }
    return row


def matrix_selection(matrix, report, protocol):
    if matrix["schema_version"] != protocol:
        raise ValueError("unsupported matrix schema")
    rows = {row["id"]: row for row in matrix["runtimes"]}
    if len(rows) != len(matrix["runtimes"]):
        raise ValueError("duplicate runtime IDs in matrix")
    selected = []
    for run in report["runs"]:
        runtime = run["runtime"]
        row = rows[runtime["id"]]
        if (row["os"] != runtime["os"] or row["maps"] * row.get("map_cycles", 1) != runtime["map_itinerary"]
                or row.get("plugin_sha256", runtime["plugin"]["sha256"]) != runtime["plugin"]["sha256"]):
            raise ValueError(f"matrix disagrees with audited itinerary or plugin for {runtime['id']}")
        public = pick(row, ("id", "os", "mod", "maps", "map_cycles"))
        for name, observed in runtime["components"].items():
            expected = row[name]
            fields = (*COMPONENT_FIELDS, "sha256")
            if any(key in expected and expected[key] != observed.get(key) for key in fields):
                raise ValueError(f"matrix {name} disagrees with audit report for {runtime['id']}")
            public[name] = pick(observed, fields)
            if name == "engine":
                modules = observed.get("modules", {})
                if set(expected.get("modules", [])) != set(modules):
                    raise ValueError(f"matrix engine modules disagree for {runtime['id']}")
                public[name]["modules"] = sorted(modules)
                public[name]["module_sha256"] = modules
                if expected.get("module_sha256", modules) != modules:
                    raise ValueError(f"matrix engine module hashes disagree for {runtime['id']}")
        public["plugin_sha256"] = runtime["plugin"]["sha256"]
        safe_text(public)
        selected.append(public)
    return {"schema_version": protocol, "runtimes": selected}


def portable_certificate(report, root, profile, protocol, parse_event):
    cert = certification.evaluate(report, root / "report.json", profile, protocol, parse_event)
    if not cert["all_certified"]:
        raise ValueError("bundle evidence does not certify every selected runtime")
    cert["report"]["path"] = "report.json"
    for row in cert["runtimes"]:
        row["evidence"]["event_log"] = f"{row['runtime_id']}/events.jsonl"
    return cert


def export(report_path, matrix_path, output, profile, protocol, parse_event, load):
    report_path = report_path.resolve(strict=True)
    report = load(report_path)
    original = certification.evaluate(report, report_path, profile, protocol, parse_event)
    if not original["all_certified"]:
        raise ValueError("export requires an existing report with every row certified")
    matrix = matrix_selection(load(matrix_path), report, protocol)
    public = pick(report, ("schema_version", "run_id"))
    safe_text(public)
    public["runs"] = [public_run(run) for run in report["runs"]]
    output = output.absolute()
    if output.exists() or output.is_symlink():
        raise ValueError("bundle output must be a new directory")
    with tempfile.TemporaryDirectory(dir=output.parent) as temp:
        root = Path(temp) / "bundle"
        root.mkdir()
        files = set(FILES)
        for row in public["runs"]:
            rid = row["runtime"]["id"]
            relative = f"{rid}/events.jsonl"
            source = report_path.parent / relative
            path = root / relative
            path.parent.mkdir()
            records = certification.event_records(row, source, parse_event)
            for event in records:
                safe_text(event)
                if event.keys() - EVENT_FIELDS - EVENT_PAYLOADS[event["kind"]]:
                    raise ValueError("unrecognized event fields may contain private data; cannot publish unchanged")
            shutil.copyfile(source, path)
            # The evaluator checks the exact copied bytes against the original digest.
            row["runtime_identity"] = certification.runtime_identity(
                row["runtime"], row["console_version_observations"], records, row["mapped_artifacts"])
            files.add(relative)
        write(root / "report.json", public)
        write(root / "matrix.json", matrix)
        cert = portable_certificate(load(root / "report.json"), root, profile, protocol, parse_event)
        write(root / "certificate.json", cert)
        (root / "certificate.md").write_text(certification.markdown(cert), encoding="utf-8")
        write(root / "manifest.json", {"schema_version": SCHEMA, "profile": profile,
                                       "source_report_sha256": original["report"]["sha256"],
                                       "files": {name: digest(root / name) for name in sorted(files)}})
        root.rename(output)


def verify(root, protocol, parse_event, load):
    if root.is_symlink() or not root.is_dir():
        raise ValueError("expected a regular bundle directory")
    actual, directories = set(), set()
    for directory, dirs, names in os.walk(root):
        for name in dirs + names:
            path = Path(directory) / name
            if path.is_symlink() or getattr(path.lstat(), "st_file_attributes", 0) & 0x400:
                raise ValueError("bundle contains a symlink or reparse point")
            relative = path.relative_to(root).as_posix()
            if path.is_dir():
                directories.add(relative)
            elif path.is_file():
                actual.add(relative)
            else:
                raise ValueError("bundle contains a special file")
    manifest = load(root / "manifest.json")
    if manifest["schema_version"] != SCHEMA or manifest["profile"] not in certification.PROFILES:
        raise ValueError("unsupported certification bundle")
    files = manifest["files"]
    if actual != set(files) | {"manifest.json"}:
        raise ValueError("bundle file inventory disagrees with manifest")
    for name, expected in files.items():
        if digest(root / name) != expected:
            raise ValueError(f"bundle hash mismatch: {name}")
    report = load(root / "report.json")
    cert = portable_certificate(report, root, manifest["profile"], protocol, parse_event)
    ids = {row["runtime_id"] for row in cert["runtimes"]}
    if directories != ids or set(files) != FILES | {f"{rid}/events.jsonl" for rid in ids}:
        raise ValueError("unexpected or missing bundle evidence paths")
    matrix = load(root / "matrix.json")
    if matrix_selection(matrix, report, protocol) != matrix:
        raise ValueError("bundled matrix disagrees with the certified rows")
    if (json.loads(json.dumps(cert)) != load(root / "certificate.json")
            or certification.markdown(cert) != (root / "certificate.md").read_text(encoding="utf-8")):
        raise ValueError("bundled certificate differs from the offline evaluation")
    return cert
