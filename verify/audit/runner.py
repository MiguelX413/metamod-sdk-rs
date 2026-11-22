#!/usr/bin/env python3
"""Run metamod-audit in disposable GoldSrc server copies (Python 3.10+)."""

import argparse
from contextlib import nullcontext
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import signal
import struct
import subprocess
import tempfile
import threading
import time
import uuid

from evidence import CONSOLE_EVIDENCE, CRASH, NATIVE_PREFIX, SANITIZER, identity, linux_mappings, windows_mappings
import certification

SCHEMA = 1
REPO = Path(__file__).resolve().parents[2]
PREFIX = b"MM_AUDIT "
STATUSES = {"PASS", "FAIL", "SKIP", "INCONCLUSIVE"}
CLASSIFICATIONS = {
    "Verified portable behavior", "Implementation-specific behavior",
    "Unverified host assumption", "SDK defect", "Host defect",
    "Unsupported configuration",
}
MAX_EVENTS = 4096
LOG_LIMIT = 64 * 1024 * 1024
SOURCE = REPO / "metamod-audit/src/lib.rs"
CONTRACTS = {
    "ffi.pointer_readability.v1": "native inputs, return buffers and hook result slots remain readable for the contracted lifetime",
    "ffi.string_reentry.v1": "native activity invisible to SDK callbacks leaves return buffers valid until copied",
    "ffi.registration_ownership.v1": "host retains or copies registration storage and only frees its own allocations",
    "ffi.edict_arena.v1": "map lifecycle callbacks precede edict-arena retirement and serials distinguish slot allocations",
    "ffi.shutdown_order.v1": "host keeps callback code and active hook globals live until native frames unwind",
}
OBSERVATION_TESTS = {"lag.post.v1", "player.identity.v1", "trace.line.v1", "trace.hull.v1"}
MAP_PROBES = {"map.deactivate.v1", "map.activate.v1", "map.order.v1", "map.stale.v1"}
NATIVE_SUITE = {
    "native.command.snapshot_limits.v1",
    "native.dispatch.reentrant.v1",
    "native.entity.reentrant_map.v1",
    "native.hook.result.v1",
    "native.shutdown.dispatch_revoked.v1",
    "native.shutdown.reentrant_trace.v1",
}
OBSERVATION_RANK = {"SKIP": 0, "INCONCLUSIVE": 1, "PASS": 2, "FAIL": 3}
PROBE_ID = re.compile(r"[a-z][a-z0-9_]*(?:\.[a-z][a-z0-9_]*)*\.v[1-9][0-9]*")


def dump(path, value):
    path.write_text(json.dumps(value, indent=2, ensure_ascii=True) + "\n", encoding="utf-8")


def digest(path):
    result = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            result.update(block)
    return result.hexdigest()


def catalog():
    return dict(re.findall(r'id: "([^"]+)",\s*expected: "([^"]+)"', SOURCE.read_text(encoding="utf-8")))


def unique_object(pairs):
    obj = dict(pairs)
    if len(obj) != len(pairs):
        raise ValueError("duplicate JSON key")
    return obj


def parse_event(line):
    # Console timestamps or engine logging prefixes may precede a record.
    _, marker, payload = line.partition(PREFIX)
    if not marker or len(payload) > 8192 or not line.endswith(b"\n"):
        return None
    try:
        event = json.loads(payload, object_pairs_hook=unique_object)
    except (ValueError, UnicodeError, RecursionError):
        return None
    if not isinstance(event, dict) or type(event.get("schema_version")) is not int or event["schema_version"] != SCHEMA:
        return None
    if type(event.get("seq")) is not int or not 1 <= event["seq"] <= MAX_EVENTS:
        return None
    kind = event.get("kind")
    if not isinstance(kind, str):
        return None
    if kind == "result":
        if (not all(isinstance(event.get(k), str) and event[k].strip()
                    for k in ("test", "result", "expected", "observed"))
                or event["result"] not in STATUSES):
            return None
        if not PROBE_ID.fullmatch(event["test"]):
            return None
    elif kind not in {"ready", "status", "report", "usage", "suite_complete", "limit"}:
        return None
    elif kind == "ready" and type(event.get("submitted")) is not bool:
        return None
    elif kind == "report" and event.get("complete") is not True:
        return None
    elif kind == "suite_complete" and event.get("suite") not in ("all", "console", "entities", "trace", "resources"):
        return None
    return event


def read_events(paths):
    events = {}
    diagnostics = []
    warnings = []
    versions = []
    conflicts = []
    native = {}
    known = catalog()
    def warn(message):
        if len(warnings) < 32:
            warnings.append(message)
    for path in paths:
        with path.open("rb") as stream:
            remaining = LOG_LIMIT
            while remaining and (line := stream.readline(min(16384, remaining))):
                remaining -= len(line)
                event = parse_event(line)
                if event:
                    seq = event["seq"]
                    if event["kind"] == "result" and event["test"] not in known:
                        warn(f"{path.name}: unknown probe identifier at sequence {seq}: {event['test'][:120]}")
                    if seq in events:
                        warn(f"{path.name}: {'duplicate' if events[seq] == event else 'conflicting'} record at sequence {seq}")
                        if events[seq] != event and len(conflicts) < 32:
                            conflicts.append(event)
                    else:
                        events[seq] = event
                elif PREFIX in line:
                    warn(f"{path.name}: invalid or truncated protocol record")
                if NATIVE_PREFIX in line:
                    _, _, payload = line.partition(NATIVE_PREFIX)
                    try:
                        result = json.loads(payload, object_pairs_hook=unique_object)
                        test = result["test"]
                        if (not line.endswith(b"\n") or type(result["schema_version"]) is not int
                                or result["schema_version"] != SCHEMA or result["result"] != "PASS"
                                or not isinstance(test, str) or not test.startswith("native.") or not PROBE_ID.fullmatch(test)
                                or not isinstance(result["observed"], str) or not result["observed"].strip()):
                            raise ValueError("invalid native record")
                        if test in native:
                            previous = {key: value for key, value in native[test].items() if key != "source"}
                            warn(f"{path.name}: {'duplicate' if previous == result else 'conflicting'} native record for {test[:120]}")
                        elif len(native) < 128:
                            native[test] = {**result, "source": "native-harness"}
                        else:
                            warn(f"{path.name}: native fixture record budget exceeded")
                    except (KeyError, TypeError, ValueError, UnicodeError, RecursionError):
                        warn(f"{path.name}: invalid or duplicate native fixture record")
                decoded = line[:2048].decode("utf-8", "replace").strip()
                if (SANITIZER.search(decoded) or CRASH.search(decoded)) and len(diagnostics) < 128:
                    diagnostics.append(decoded)
                if CONSOLE_EVIDENCE.search(decoded) and len(versions) < 32 and decoded not in versions:
                    versions.append(decoded)
            if not remaining and stream.read(1):
                warn(f"{path.name}: log exceeded capture budget")
    if events and len(events) != max(events):
        warnings.append("one or more event sequence numbers are missing")
    return [events[i] for i in sorted(events)], conflicts, diagnostics, warnings, versions, list(native.values())


def findings(events, executed, manual, unsupported=None):
    tests = catalog()
    results = {}
    history = {}
    for event in events:
        if event["kind"] != "result":
            continue
        test = event["test"]
        if test not in tests:
            continue
        history.setdefault(test, []).append(event["seq"])
        previous = results.get(test, {}).get("result")
        if previous != "FAIL" and OBSERVATION_RANK[event["result"]] >= OBSERVATION_RANK.get(previous, -1):
            results[test] = {**event, "source": "plugin"}
    for item in manual:
        test = item["test"]
        if test in CONTRACTS:
            continue
        if results.get(test, {}).get("result") in {"PASS", "FAIL"}:
            if item["result"] == results[test]["result"]:
                results[test]["manual_evidence"] = item["observed"]
            elif item["result"] in {"PASS", "FAIL"}:
                results[test]["manual_evidence"] = item["observed"]
                results[test]["manual_conflict"] = item["result"]
            continue
        results[test] = {**item, "expected": tests[test], "source": "manual"}
    for test, expected in tests.items():
        results.setdefault(test, {
            "test": test, "expected": expected, "result": "SKIP", "source": "runner",
            "observed": unsupported or ("no observation recorded" if executed else "runtime did not execute"),
            "classification": "Unsupported configuration" if unsupported else "Unverified host assumption",
        })
    for test, result in results.items():
        result["event_sequences"] = history.get(test, [])
        result.setdefault("classification", "Unverified host assumption")
        if result["result"] in {"PASS", "FAIL"} and result["source"] == "plugin":
            result["classification"] = "Implementation-specific behavior"
    return sorted(results.values(), key=lambda item: item["test"])


def manual_results(path):
    if path is None:
        return []
    records = json.loads(path.read_text(encoding="utf-8"))
    seen = set()
    for item in records:
        if (item["test"] not in catalog() | CONTRACTS or item["test"] in seen or item["result"] not in STATUSES
                or item["classification"] not in CLASSIFICATIONS or not item["observed"].strip()
                or not item["operator"].strip()):
            raise ValueError("invalid manual finding (require unique known test, result, classification, evidence and operator)")
        seen.add(item["test"])
    return records


def aggregate(output, runtime, metadata, logs, manual=()):
    events, conflicts, diagnostics, warnings, versions, native = read_events(logs)
    scope = runtime.get("evidence_scope", "runtime")
    if native and scope != "native-fixture":
        warnings.append("native fixture records present in a runtime transcript")
    if metadata.get("require_native_suite"):
        observed = {item["test"] for item in native}
        if missing := NATIVE_SUITE - observed:
            warnings.append("missing native fixture evidence: " + ", ".join(sorted(missing)))
        if unexpected := observed - NATIVE_SUITE:
            warnings.append("unexpected native fixture evidence: " + ", ".join(sorted(unexpected)))
    if any(event["kind"] == "limit" for event in events):
        warnings.append("plugin event budget reached; later observations may be missing")
    records = [(event, False) for event in events] + [(event, True) for event in conflicts]
    with (output / "events.jsonl").open("w", encoding="utf-8") as stream:
        for event, conflict in sorted(records, key=lambda record: record[0]["seq"]):
            record = {**event, "run_id": metadata["run_id"], "runtime_id": runtime["id"],
                      "evidence_scope": scope, "sequence_conflict": conflict}
            stream.write(json.dumps(record) + "\n")
    failures = [event for event in conflicts if event.get("result") == "FAIL"]
    results = findings(events + failures, metadata["executed"], manual, runtime.get("unsupported_reason"))
    if scope == "native-fixture":
        for result in results:
            result["classification"] = "Synthetic fixture behavior" if result["source"] != "runner" else result["classification"]
    contract_evidence = {item["test"]: item for item in manual if item["test"] in CONTRACTS}
    ready = [event for event in events if event["kind"] == "ready"]
    if len(ready) > 1:
        warnings.append("multiple readiness records in a single plugin session")
    ready_seq = ready[0]["seq"] if len(ready) == 1 and ready[0]["submitted"] else MAX_EVENTS
    status_seq = next((event["seq"] for event in events
                       if event["kind"] == "status" and event["seq"] > ready_seq), MAX_EVENTS)
    suite_seq = next((event["seq"] for event in events if event["seq"] > status_seq
                      and event["kind"] == "suite_complete" and event.get("suite") == "all"), MAX_EVENTS)
    report_seq = next((event["seq"] for event in events if event["seq"] > suite_seq and event["kind"] == "report"), None)
    if metadata.get("require_native_suite"):
        shutdown = {test: [event["seq"] for event in events if event.get("test") == test and event.get("result") == "PASS"]
                    for test in ("shutdown.v1", "shutdown.drop.v1")}
        hook, destructor = shutdown["shutdown.v1"], shutdown["shutdown.drop.v1"]
        if report_seq is None or len(hook) != 1 or len(destructor) != 1 or not report_seq < hook[0] < destructor[0]:
            warnings.append("missing, repeated, or out-of-order audit plugin shutdown evidence in native fixture")
    protocol_complete = (not warnings and not metadata.get("capture_errors") and not metadata.get("truncated_streams")
                         and report_seq is not None)
    runtime_identity = identity(runtime, versions, events, metadata.get("mapped_artifacts", {})) if scope == "runtime" else {}
    return {
        "schema_version": SCHEMA, "runtime": runtime, **metadata,
        "evidence_scope": scope, "runtime_identity": runtime_identity,
        "identity_failures": [name for name, result in runtime_identity.items() if result["result"] == "FAIL"],
        "native_fixture_results": native if scope == "native-fixture" else [],
        "sanitizer_diagnostics": [line for line in diagnostics if SANITIZER.search(line)],
        "crash_diagnostics": [line for line in diagnostics if CRASH.search(line)],
        "protocol_complete": protocol_complete,
        "console_version_observations": versions,
        "diagnostics": diagnostics, "protocol_warnings": warnings,
        "event_log": str(output / "events.jsonl"), "tests": results,
        "event_log_sha256": digest(output / "events.jsonl"),
        "contract_findings": [{"id": contract, "classification": "Unverified host assumption",
                               "expected": expected, "observed": "requires evidence beyond public API behavior",
                               **contract_evidence.get(contract, {})}
                              for contract, expected in CONTRACTS.items()],
    }


def markdown(report):
    lines = ["# GoldSrc runtime audit", "", f"Run `{report['run_id']}`", ""]
    for run in report["runs"]:
        runtime = run["runtime"]
        lines.extend([f"## {runtime['id']}", "",
                      f"Execution **{run['execution']}**, exit status `{run['exit_status']}`, "
                      f"console protocol {'complete' if run['protocol_complete'] else 'incomplete'}", ""])
        for component, identity in runtime.get("components", {}).items():
            lines.append(f"- {component} — {identity['implementation']} {identity['version']} (`{identity.get('sha256', 'unavailable')}`)")
        if plugin := runtime.get("plugin"):
            lines.append(f"- Plugin SHA-256 `{plugin['sha256']}`, audit commit `{plugin.get('audit_plugin_commit')}`, SDK commit `{plugin.get('sdk_commit')}`")
        if run.get("error"):
            lines.extend(["", str(run["error"])])
        lines.extend(["", f"Evidence scope **{run['evidence_scope']}**"])
        for component, result in run["runtime_identity"].items():
            reasons = "; ".join(result["reasons"])
            lines.append(f"- {component} runtime identity **{result['result']}** {reasons}")
        for result in run["native_fixture_results"]:
            lines.append(f"- `{result['test']}` **{result['result']}** {result['observed']}")
        lines.extend(["", "| Test | Result | Classification | Evidence |", "| --- | --- | --- | --- |"])
        for test in run["tests"]:
            evidence = test["observed"].replace("|", r"\|").replace("\n", " ")
            lines.append(f"| `{test['test']}` | {test['result']} | {test['classification']} | {evidence} |")
        if run.get("diagnostics"):
            lines.extend(["", "Diagnostics are recorded in the JSON report and raw console logs."])
        if run["protocol_warnings"]:
            lines.extend(["", *[f"- {warning}" for warning in run["protocol_warnings"]]])
        lines.extend(["", "Native ownership and lifetime assumptions remain recorded separately in the JSON report."])
        lines.append("")
    return "\n".join(lines)


def finish(output, run_id, runs):
    result = {"schema_version": SCHEMA, "run_id": run_id, "runs": runs}
    dump(output / "report.json", result)
    (output / "report.md").write_text(markdown(result), encoding="utf-8")
    return result


def inside(root, name):
    candidate = root / name
    resolved = candidate.resolve()
    if Path(name).is_absolute() or not resolved.is_relative_to(root):
        raise ValueError(f"path outside installation: {name}")
    if candidate != resolved:
        raise ValueError(f"noncanonical installation path: {name}")
    return resolved


def check_binary(path, system, *, library=False):
    with path.open("rb") as stream:
        header = stream.read(64)
        if system == "linux":
            valid = (header[:6] == b"\x7fELF\x01\x01" and len(header) >= 20
                     and header[18:20] == b"\x03\x00"
                     and (not library or header[16:18] == b"\x03\x00"))
        else:
            valid = False
            if header[:2] == b"MZ" and len(header) == 64:
                stream.seek(struct.unpack_from("<I", header, 60)[0])
                pe = stream.read(26)
                valid = (len(pe) == 26 and pe[:6] == b"PE\0\0\x4c\x01"
                         and pe[24:26] == b"\x0b\x01"
                         and (not library or bool(pe[23] & 0x20)))
    if not valid:
        raise ValueError(f"expected 32-bit x86 {system} binary: {path}")


def version_metadata():
    def git(*args):
        return subprocess.check_output(["git", *args], cwd=REPO)
    try:
        revision = git("rev-parse", "HEAD").decode().strip()
        return {
            "audit_plugin_commit": revision,
            "sdk_commit": revision,
            "working_tree_dirty": bool(git("status", "--porcelain", "--untracked-files=normal")),
        }
    except (OSError, subprocess.CalledProcessError):
        return {"audit_plugin_commit": None, "sdk_commit": None}


def label(value, what):
    if not re.fullmatch(r"[A-Za-z0-9_-]{1,64}", value):
        raise ValueError(f"invalid {what}: {value!r}")
    return value


class Console:
    def __init__(self, process, output):
        self.process = process
        self.output = output
        self.threads = []
        self.condition = threading.Condition()
        self.events = []
        self.errors = []
        self.truncated = []
        for name in ("stdout", "stderr"):
            thread = threading.Thread(target=self.read, args=(name,), daemon=True)
            thread.start()
            self.threads.append(thread)

    def read(self, name):
        try:
            with (self.output / f"{name}.log").open("wb") as log:
                stream = getattr(self.process, name)
                remaining = LOG_LIMIT
                while line := stream.readline(16384):
                    written = log.write(line[:remaining])
                    remaining -= written
                    if len(line) > written:
                        if name not in self.truncated:
                            self.truncated.append(name)
                    event = parse_event(line)
                    if event:
                        with self.condition:
                            if len(self.events) < MAX_EVENTS * 2:
                                self.events.append(event)
                            self.condition.notify_all()
        except OSError as error:
            self.errors.append(str(error))

    def sequence(self):
        with self.condition:
            return max((event["seq"] for event in self.events), default=0)

    def between(self, start, end):
        with self.condition:
            return [event for event in self.events if start < event["seq"] <= end]

    def wait(self, kind, timeout, after=0, **fields):
        deadline = time.monotonic() + timeout
        with self.condition:
            while True:
                for event in self.events:
                    if event["seq"] > after and event["kind"] == kind and all(event.get(k) == v for k, v in fields.items()):
                        return event
                remaining = deadline - time.monotonic()
                if remaining <= 0 or self.process.poll() is not None:
                    raise RuntimeError(f"did not observe {kind}; exit={self.process.poll()}")
                self.condition.wait(min(remaining, 0.25))

    def send(self, line):
        data = (line + "\n").encode("ascii")
        if len(data) > 512:
            raise ValueError("console command exceeds bounded write")
        self.process.stdin.write(data)
        self.process.stdin.flush()


def stop(process, console, timeout):
    if process.poll() is not None:
        return "already exited"
    try:
        console.send("quit")
        process.wait(timeout=timeout)
        return "quit"
    except (OSError, subprocess.TimeoutExpired):
        if process.poll() is None:
            if os.name == "posix":
                try:
                    os.killpg(process.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
            else:
                process.terminate()
        try:
            process.wait(timeout=5)
            return "terminated"
        except subprocess.TimeoutExpired:
            if os.name == "posix":
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            else:
                process.kill()
            process.wait(timeout=5)
            return "killed"


def configure(stage, spec):
    mod = inside(stage, spec["mod"])
    liblist = mod / "liblist.gam"
    audit_dir = mod / "addons" / "metamod-audit"
    if audit_dir.exists():
        raise ValueError("installation already contains the reserved addons/metamod-audit directory")
    audit_dir.mkdir(parents=True)
    system = spec["os"]
    name = "metamod_audit.dll" if system == "windows" else "libmetamod_audit.so"
    deployed_plugin = audit_dir / name
    shutil.copy2(spec["_plugin"], deployed_plugin)
    manager = inside(stage, spec["metamod"]["binary"]).relative_to(mod).as_posix()
    game = inside(stage, spec["gamedll"]["binary"]).relative_to(mod).as_posix()
    if not all(re.fullmatch(r"[A-Za-z0-9_./-]+", path) for path in (manager, game)):
        raise ValueError("manager and GameDLL paths must be console-safe paths inside the mod")
    original = liblist.read_bytes()
    key = b"gamedll" if system == "windows" else b"gamedll_linux"
    replacement = key + b' "' + manager.encode("ascii") + b'"'
    patched, count = re.subn(rb"(?im)^[ \t]*" + key + rb"[ \t]+[^\r\n]*", replacement, original)
    if count != 1:
        raise ValueError("liblist.gam must contain exactly one active platform GameDLL entry")
    liblist.write_bytes(patched)
    plugin_file = "addons/metamod-audit/plugins.ini"
    (mod / plugin_file).write_text(f"{'win32' if system == 'windows' else 'linux'} addons/metamod-audit/{name}\n", encoding="ascii")
    # An isolated Metamod configuration prevents copied options from
    # redirecting logging, plugin loading, or GameDLL resolution.
    config_file = "addons/metamod-audit/config.ini"
    (mod / config_file).write_text(f"gamedll {game}\n", encoding="ascii")
    return config_file, plugin_file, game, deployed_plugin


def is_dump_header(header, size):
    if header.startswith(b"MDMP") and len(header) >= 32:
        version, streams, directory = struct.unpack_from("<III", header, 4)
        return version & 0xffff == 0xa793 and streams > 0 and 32 <= directory <= size - streams * 12
    if not header.startswith(b"\x7fELF") or len(header) < 16:
        return False
    layout = {1: (52, 40), 2: (64, 52)}.get(header[4])
    endian = {1: "<", 2: ">"}.get(header[5])
    if layout is None or endian is None or header[6] != 1:
        return False
    header_size, size_offset = layout
    if len(header) < header_size:
        return False
    kind, machine, version = struct.unpack_from(endian + "HHI", header, 16)
    return (kind == 4 and machine != 0 and version == 1
            and struct.unpack_from(endian + "H", header, size_offset)[0] == header_size)


def preserve_dumps(stage, output):
    saved = []
    for path in stage.rglob("*"):
        if len(saved) >= 8:
            break
        if path.is_symlink() or not path.is_file():
            continue
        if path.suffix.casefold() not in {".mdmp", ".dmp", ".core"} and path.name != "core" and not path.name.startswith("core."):
            continue
        size = path.stat().st_size
        if size > 256 * 1024 * 1024:
            continue
        with path.open("rb") as stream:
            if not is_dump_header(stream.read(64), size):
                continue
        target = output / f"dump-{len(saved)}-{path.name}"
        shutil.copy2(path, target)
        saved.append(target.name)
    return saved


def sample_rss(process, checkpoint):
    try:
        text = Path(f"/proc/{process.pid}/status").read_text(encoding="ascii")
        rss = re.search(r"^VmRSS:\s+(\d+) kB$", text, re.M)
        return {"checkpoint": checkpoint, "rss_kib": int(rss[1]) if rss else None}
    except (OSError, UnicodeError):
        return {"checkpoint": checkpoint, "rss_kib": None}


def run_one(spec, config_dir, output, run_id, allow_build, client_wait, prepare_only=False):
    runtime = {key: spec[key] for key in ("id", "os") if key in spec}
    runtime["architecture"] = "i686" if spec.get("os") == "linux" else "x86"
    runtime["host"] = {"system": platform.system(), "release": platform.release(),
                       "version": platform.version(), "machine": platform.machine()}
    if runtime["host"]["system"] == "Linux":
        try:
            os_release = platform.freedesktop_os_release()
            runtime["host"]["distribution"] = {key: os_release[key] for key in ("ID", "VERSION_ID")
                                               if key in os_release}
        except OSError:
            pass  # Some Linux hosts have no distribution metadata.
    runtime["components"] = {}
    runtime["experimental"] = spec.get("experimental", False)
    revision = version_metadata()
    runtime["runner_revision"] = {"commit": revision.get("sdk_commit"),
                                  "working_tree_dirty": revision.get("working_tree_dirty")}
    metadata = {"run_id": run_id, "executed": False, "execution": "MISSING", "exit_status": None}
    if reason := spec.get("unsupported_reason"):
        runtime["unsupported_reason"] = reason
        metadata.update(execution="UNSUPPORTED", error=reason)
        return aggregate(output, runtime, metadata, [])
    try:
        system = spec["os"]
        if system not in {"linux", "windows"}:
            raise ValueError(f"unsupported OS {system}")
        if platform.system().lower() != system:
            raise ValueError(f"requires a {system} host")
        installation = (config_dir / spec["installation"]).resolve(strict=True)
        if output.resolve().is_relative_to(installation):
            raise ValueError("output directory is inside the source installation")
        mod = inside(installation, spec["mod"])
        label(spec["mod"], "mod directory")
        if not (mod / "liblist.gam").is_file():
            raise ValueError("mod is missing liblist.gam")
        for component in ("engine", "metamod", "gamedll"):
            config = spec[component]
            if (not config["implementation"].strip() or not config["version"].strip()
                    or config["version"] == "SET_EXACT_VERSION"):
                raise ValueError(f"{component} requires an independently identified implementation and exact version")
            path = inside(installation, config["binary"])
            check_binary(path, system, library=component != "engine")
            actual_hash = digest(path)
            if config.get("sha256", actual_hash).lower() != actual_hash:
                raise ValueError(f"{component} hash differs from the configured artifact")
            runtime["components"][component] = {**config, "sha256": actual_hash, "version_source": "operator configuration"}
        # Some engines use a launcher and a separate engine module. Hash any
        # such modules independently instead of conflating them with the exe.
        modules = {}
        for name in spec["engine"].get("modules", []):
            module = inside(installation, name)
            check_binary(module, system, library=True)
            modules[name] = digest(module)
        runtime["components"]["engine"]["modules"] = modules
        lib_dirs = spec["engine"].get("library_dirs", [])
        if not all(inside(installation, path).is_dir() for path in lib_dirs):
            raise ValueError("engine.library_dirs must point to directories within the installation")
        maps = spec["maps"]
        if not 1 <= len(maps) <= 8:
            raise ValueError("configure one to eight maps")
        for name in maps:
            label(name, "map")
            if not (mod / "maps" / (name + ".bsp")).is_file() and not (installation / "valve" / "maps" / (name + ".bsp")).is_file():
                raise ValueError(f"map is missing: {name}")
        cycles = spec.get("map_cycles", 1)
        if type(cycles) is not int or not 1 <= cycles <= 8:
            raise ValueError("map_cycles must be an integer in 1..8")
        itinerary = maps * cycles
        runtime["map_itinerary"] = itinerary
        port = int(spec.get("port", 27045))
        if not 1024 <= port <= 65535:
            raise ValueError("port must be in 1024..65535")
        timeout = int(spec.get("timeout_seconds", 45))
        if not 1 <= timeout <= 300:
            raise ValueError("timeout_seconds must be in 1..300")
        target = "i686-unknown-linux-gnu" if system == "linux" else "i686-pc-windows-msvc"
        plugin_name = "libmetamod_audit.so" if system == "linux" else "metamod_audit.dll"
        if "plugin" in spec:
            plugin = (config_dir / spec["plugin"]).resolve(strict=True)
            provenance = {key: spec.get(key) for key in ("audit_plugin_commit", "sdk_commit")}
            if "plugin_provenance" in spec:
                provenance["artifact_manifest"] = spec["plugin_provenance"]
        elif allow_build:
            provenance = version_metadata()
            provenance["built_by_runner"] = True
            build_target = output.resolve() / "build"
            command = ["cargo", "build", "--locked", "--release", "--target", target, "-p", "metamod-audit", "--target-dir", str(build_target)]
            with (output / "build.log").open("wb") as log:
                subprocess.run(command, cwd=REPO, stdout=log, stderr=subprocess.STDOUT, check=True, timeout=600)
            plugin = build_target / target / "release" / plugin_name
        else:
            plugin = REPO / "target" / target / "release" / plugin_name
            provenance = {key: spec.get(key) for key in ("audit_plugin_commit", "sdk_commit")}
        check_binary(plugin, system, library=True)
        plugin_hash = digest(plugin)
        if spec.get("plugin_sha256", plugin_hash).lower() != plugin_hash:
            raise ValueError("plugin hash differs from the configured artifact")
        runtime["plugin"] = {**provenance, "implementation": "metamod-audit", "binary": plugin.name,
                             "sha256": plugin_hash, "target": target}
        # Dereferencing symlinks during copy could escape the source tree.
        # Reject junctions too, including on Python versions without is_junction.
        for directory, dirs, files in os.walk(installation):
            for name in dirs + files:
                entry = Path(directory) / name
                stat = entry.lstat()
                if entry.is_symlink() or getattr(stat, "st_file_attributes", 0) & 0x400:
                    raise ValueError(f"source installation contains a symlink or reparse point: {entry}")
                if not entry.is_dir() and not entry.is_file():
                    raise ValueError(f"source installation contains a special file: {entry}")
        container = nullcontext(output) if prepare_only else tempfile.TemporaryDirectory(prefix="goldsrc-audit-", dir=output)
        with container as temp:
            stage = Path(temp).resolve() / "server"
            runtime["staged_root"] = str(stage)
            shutil.copytree(installation, stage)
            settings, plugins, game, deployed_plugin = configure(stage, {**spec, "_plugin": plugin})
            for identity in runtime["components"].values():
                if digest(inside(stage, identity["binary"])) != identity["sha256"]:
                    raise ValueError(f"source changed while staging {identity['binary']}")
            for name, expected_hash in modules.items():
                if digest(inside(stage, name)) != expected_hash:
                    raise ValueError(f"source changed while staging {name}")
            if digest(deployed_plugin) != plugin_hash:
                raise ValueError("plugin changed while staging")
            binary = inside(stage, spec["engine"]["binary"])
            args = [str(binary), "-console", "-insecure", "-nomaster", "-game", spec["mod"],
                    "-port", str(port), "+ip", "127.0.0.1", "+sv_lan", "1", "+deathmatch", "1",
                    "+localinfo", "mm_configfile", settings, "+localinfo", "mm_pluginsfile", plugins,
                    "+localinfo", "mm_gamedll", game, "+map", maps[0]]
            env = os.environ.copy()
            launch_env = {}
            if system == "linux":
                launch_env["LD_LIBRARY_PATH"] = os.pathsep.join([str(stage), *(str(inside(stage, path)) for path in lib_dirs)])
            elif lib_dirs:
                launch_env["PATH"] = os.pathsep.join([*(str(inside(stage, path)) for path in lib_dirs), env["PATH"]])
            env.update(launch_env)
            if prepare_only:
                args.insert(2, "-condebug")
            dump(output / "launch.json", {"argv": args, "cwd": str(stage), "environment": launch_env})
            if prepare_only:
                metadata["execution"] = "PREPARED"
                dump(output / "runtime.json", runtime)
                return aggregate(output, runtime, metadata, [])
            with subprocess.Popen(args, cwd=stage, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=subprocess.PIPE, bufsize=0, start_new_session=(system == "linux")) as process:
                console = Console(process, output)
                metadata.update(executed=True, execution="INCOMPLETE")
                metadata["resource_samples"] = []
                metadata["map_transitions"] = []
                metadata["map_visits"] = []
                try:
                    console.wait("ready", timeout, submitted=True)
                    before = console.sequence()
                    console.send("version")
                    console.send("meta version")
                    console.send("meta list")
                    console.send("meta game")
                    console.send("mm_audit_status")
                    console.wait("status", timeout, after=before)
                    expected = {key: [inside(stage, value["binary"])]
                                for key, value in runtime["components"].items()}
                    expected["engine"].extend(inside(stage, name) for name in modules)
                    expected["plugin"] = [deployed_plugin]
                    inspect_mappings = linux_mappings if system == "linux" else windows_mappings
                    metadata["mapped_artifacts"] = inspect_mappings(process.pid, expected)
                    metadata["resource_samples"].append(sample_rss(process, "ready"))
                    for index, name in enumerate(itinerary):
                        if index:
                            before = console.sequence()
                            transition = {"index": index, "requested_map": name, "result": "INCONCLUSIVE"}
                            metadata["map_transitions"].append(transition)
                            metadata["resource_samples"].append(sample_rss(process, f"before-change:{index}:{name}"))
                            console.send(f"changelevel {name}")
                            activation = console.wait("result", timeout, after=before, test="map.activate.v1", result="PASS")
                            change_start = before
                            lifecycle = console.between(change_start, activation["seq"])
                            transition["callback_sequences"] = [event["seq"] for event in lifecycle
                                                               if event.get("test") in MAP_PROBES]
                            transition["deactivated"] = any(event.get("test") == "map.deactivate.v1" and
                                                            event["result"] == "PASS" for event in lifecycle)
                        before = console.sequence()
                        console.send('mm_audit_run all "audit two words"')
                        suite = console.wait("suite_complete", timeout, after=before, suite="all")
                        metadata["map_visits"].append({"index": index, "requested_map": name,
                                                       "after_sequence": before, "suite_sequence": suite["seq"]})
                        if index:
                            related = [event for event in console.between(change_start, suite["seq"])
                                       if event.get("test") in MAP_PROBES]
                            transition["callback_sequences"] = [event["seq"] for event in related]
                            transition["suite_completed"] = True
                            transition["previous_handle_checks"] = [
                                event["result"] for event in related if event["test"] == "map.stale.v1"
                            ]
                            if any(event["result"] == "FAIL" for event in related):
                                transition["result"] = "FAIL"
                            elif transition["deactivated"]:
                                transition["result"] = "PASS"
                        metadata["resource_samples"].append(sample_rss(process, f"suite:{index}:{name}"))
                    if client_wait:
                        print(f"{runtime['id']}: connect to 127.0.0.1:{port} now; waiting {client_wait} seconds", flush=True)
                        deadline = time.monotonic() + client_wait
                        while time.monotonic() < deadline and process.poll() is None:
                            time.sleep(min(1, max(0, deadline - time.monotonic())))
                    before = console.sequence()
                    console.send("mm_audit_report")
                    console.wait("report", timeout, after=before)
                    metadata["resource_samples"].append(sample_rss(process, "before-quit"))
                    metadata["execution"] = "COMPLETE"
                except (OSError, RuntimeError) as error:
                    metadata["error"] = str(error)
                finally:
                    metadata["shutdown_method"] = stop(process, console, min(timeout, 15))
                    metadata["exit_status"] = process.returncode
                    for thread in console.threads:
                        thread.join(timeout=5)
                    metadata["capture_errors"] = console.errors + [
                        "console stream did not close" for thread in console.threads if thread.is_alive()
                    ]
                    metadata["truncated_streams"] = console.truncated
                if (process.returncode != 0 or metadata["shutdown_method"] != "quit"
                        or metadata["capture_errors"] or console.truncated):
                    metadata["execution"] = "INCOMPLETE"
            metadata["crash_dumps"] = preserve_dumps(stage, output)
    except (OSError, ValueError, KeyError, RuntimeError, subprocess.SubprocessError) as error:
        metadata["error"] = str(error)
        if not metadata["executed"]:
            metadata["execution"] = "UNAVAILABLE"
        else:
            metadata["execution"] = "INCOMPLETE"
    logs = [path for path in (output / "stdout.log", output / "stderr.log") if path.exists()]
    return aggregate(output, runtime, metadata, logs)


def check_allowances(allowances):
    if not isinstance(allowances, list):
        raise ValueError("comparison allowances must be a JSON array")
    for rule in allowances:
        if (not isinstance(rule, dict) or not {"test", "before", "after"} <= rule.keys()
                or not isinstance(rule.get("reason"), str) or not rule["reason"].strip()):
            raise ValueError("every comparison allowance requires a test, transition, and explicit reason")


def comparison_probes(run):
    manual = [test for test in run.get("contract_findings", []) if "test" in test and "result" in test]
    return run["tests"] + run.get("native_fixture_results", []) + manual


def compare(baseline, candidate, allowances):
    check_allowances(allowances)

    def runs(report):
        return {(run.get("evidence_scope", "runtime"), run["runtime"]["id"]): run for run in report["runs"]}

    def indexed(report):
        return {(*key, test["test"]): test for key, run in runs(report).items()
                for test in comparison_probes(run)}
    old_runs, new_runs = runs(baseline), runs(candidate)
    differences = []
    drift = set()
    for key in sorted(old_runs.keys() | new_runs.keys()):
        scope, runtime = key
        old, new = old_runs.get(key, {}), new_runs.get(key, {})
        def change(field, before, after, category):
            if before != after:
                differences.append({"runtime": runtime, "evidence_scope": scope, "field": field,
                                    "before": before, "after": after, "category": category,
                                    **comparison_status(category, key in drift)})

        def host_config(run):
            info = run.get("runtime", {})
            fields = ["os", "architecture", "components"]
            if all("map_itinerary" in record.get("runtime", {}) for record in (old, new)):
                fields.append("map_itinerary")
            return {field: info.get(field) for field in fields}

        if old and new and host_config(old) != host_config(new):
            drift.add(key)
            change("host_configuration", host_config(old), host_config(new), "configuration drift")
        change("audit_plugin", old.get("runtime", {}).get("plugin"), new.get("runtime", {}).get("plugin"), "artifact change")
        if old and not new:
            change("runtime_present", True, False, "coverage loss")
        def diagnostic_keys(lines):
            return {re.sub(r"0x[0-9a-fA-F]+|==\d+==", "<address-or-pid>", line) for line in lines or []}

        for field in ("execution", "exit_status", "shutdown_method", "protocol_complete", "protocol_warnings",
                      "sanitizer_diagnostics", "crash_diagnostics", "diagnostics", "capture_errors",
                      "truncated_streams", "crash_dumps"):
            before, after = old.get(field), new.get(field)
            if field == "protocol_complete":
                expects_protocol = scope == "runtime" or new.get("require_native_suite")
                worse = after is False and (before is True or (not old and new.get("executed") and expects_protocol))
            elif field == "exit_status":
                worse = after not in (0, None)
            elif field == "execution":
                worse = (before in {"COMPLETE", "IMPORTED"} and after not in {"COMPLETE", "IMPORTED"}
                         or not old and after == "INCOMPLETE")
            elif field == "shutdown_method":
                worse = before == "quit" and after not in {"quit", None}
            elif field in {"sanitizer_diagnostics", "crash_diagnostics", "diagnostics"}:
                worse = bool(diagnostic_keys(after) - diagnostic_keys(before))
            else:
                worse = bool(set(after or []) - set(before or []))
            evidence_lost = bool(new) and after is None and (
                (field == "protocol_complete" and before is True) or (field == "exit_status" and before == 0)
            )
            change(field, before, after, "regression" if worse else "coverage loss" if evidence_lost else "execution difference")
        def observed_identity(run):
            root = run.get("runtime", {}).get("staged_root", "")
            def normalized(path):
                return path.replace(root, "<stage>", 1) if root and path.startswith(root) else path
            return {name: {**{field: info.get(field) for field in ("result", "observed_versions", "version_banners",
                                                                  "version_fields", "listed_running", "protocol_reply")},
                           "reported_paths": [normalized(path) for path in info.get("reported_paths", [])],
                           "mappings": {normalized(path): present for path, present in info.get("mapped_artifacts", {}).items()}}
                    for name, info in run.get("runtime_identity", {}).items()}
        old_identity, new_identity = observed_identity(old), observed_identity(new)
        bad_identity = (any(info["result"] == "FAIL" for info in new_identity.values())
                        or any(info["result"] == "PASS" and new_identity.get(name, {}).get("result") != "PASS"
                               for name, info in old_identity.items()))
        change("runtime_identity", old_identity, new_identity, "regression" if bad_identity else "identity difference")
        def transitions(run):
            return [(item["result"], item.get("previous_handle_checks", [])) for item in run.get("map_transitions", [])]
        old_transitions, new_transitions = transitions(old), transitions(new)
        lifecycle_failure = any(after[0] == "FAIL" and (index >= len(old_transitions) or old_transitions[index][0] != "FAIL")
                                for index, after in enumerate(new_transitions))
        lifecycle_loss = (len(new_transitions) < len(old_transitions)
                          or any((before[0] == "PASS" and after[0] != "PASS")
                                 or ("PASS" in before[1] and "PASS" not in after[1])
                                 for before, after in zip(old_transitions, new_transitions)))
        change("map_transitions", old_transitions, new_transitions,
               "regression" if lifecycle_failure else "coverage loss" if lifecycle_loss else "lifecycle difference")
        def rss_delta(run):
            samples = [item["rss_kib"] for item in run.get("resource_samples", [])
                       if item["rss_kib"] is not None]
            return samples[-1] - samples[0] if len(samples) > 1 else None
        old_delta, new_delta = rss_delta(old), rss_delta(new)
        if old_delta is not None and new_delta is not None and abs(new_delta - old_delta) >= 4096:
            change("rss_delta_kib", old_delta, new_delta, "resource observation")

    before, after = indexed(baseline), indexed(candidate)
    for key in sorted(before.keys() | after.keys()):
        scope, runtime, test = key
        old_item, new_item = before.get(key, {}), after.get(key, {})
        old, new = old_item.get("result"), new_item.get("result")
        if old == new:
            if test in OBSERVATION_TESTS and old_item.get("observed") != new_item.get("observed"):
                differences.append({"runtime": runtime, "evidence_scope": scope, "test": test,
                                    "before": old_item.get("observed"), "after": new_item.get("observed"),
                                    "category": "observation difference",
                                    **comparison_status("observation difference", (scope, runtime) in drift)})
            continue
        reason = next((rule["reason"] for rule in allowances
                       if (rule.get("runtime"), rule["test"], rule["before"], rule["after"]) == (runtime, test, old, new)), None)
        category = ("regression" if old != "FAIL" and new == "FAIL"
                    else "coverage loss" if old in {"PASS", "FAIL"} and new not in {"PASS", "FAIL"}
                    else "change")
        differences.append({"runtime": runtime, "evidence_scope": scope, "test": test, "before": old, "after": new,
                            "before_evidence": old_item.get("observed"), "after_evidence": new_item.get("observed"),
                            "category": "expected" if reason else "incomparable probe change" if (scope, runtime) in drift else category,
                            "unadjusted_category": category, "reason": reason,
                            **comparison_status(category, (scope, runtime) in drift)})
    return differences


def compare_runtimes(report, allowances):
    check_allowances(allowances)
    runs = report["runs"]
    differences = []
    # A matrix has no temporal baseline. Surface each failing row even
    # when it is first, is alone, or fails identically to another row.
    for run in runs:
        for delta in compare({"runs": []}, {"runs": [run]}, []):
            if delta.get("severity") == "failure":
                differences.append({**delta, "category": "runtime failure", "comparability": "no historical baseline",
                                    "attribution": "undetermined"})
    for index, left in enumerate(runs):
        for right in runs[index + 1:]:
            if left.get("evidence_scope", "runtime") != right.get("evidence_scope", "runtime"):
                continue
            left_id, right_id = left["runtime"]["id"], right["runtime"]["id"]
            # Reuse field-level comparison without treating one runtime
            # as a historical baseline for the other.
            aligned = {**right, "runtime": {**right["runtime"], "id": left_id}}
            field_changes = compare({"runs": [left]}, {"runs": [aligned]}, [])
            drift = any(delta.get("field") == "host_configuration" for delta in field_changes)
            for delta in field_changes:
                if "field" in delta:
                    differences.append({**{k: v for k, v in delta.items() if k not in {"runtime", "severity", "attribution"}},
                                        "left": left_id, "right": right_id, "category": "runtime difference",
                                        **comparison_status("runtime difference", drift)})
            old = {test["test"]: test for test in comparison_probes(left)}
            new = {test["test"]: test for test in comparison_probes(right)}
            for test in sorted(old.keys() & new.keys()):
                before, after = old[test]["result"], new[test]["result"]
                if before == after:
                    if (before == "PASS" and test in OBSERVATION_TESTS
                            and old[test]["observed"] != new[test]["observed"]):
                        differences.append({"left": left_id, "right": right_id, "test": test,
                                            "before": old[test]["observed"], "after": new[test]["observed"],
                                            "category": "observation difference", "reason": "both probes passed",
                                            **comparison_status("observation difference", drift)})
                    continue
                reason = next((rule["reason"] for rule in allowances
                               if (rule.get("left"), rule.get("right"), rule["test"], rule["before"], rule["after"])
                               == (left_id, right_id, test, before, after)), None)
                differences.append({"left": left_id, "right": right_id, "test": test,
                                    "before": before, "after": after,
                                    "before_evidence": old[test].get("observed"),
                                    "after_evidence": new[test].get("observed"),
                                    "category": "expected" if reason else "runtime difference", "reason": reason,
                                    **comparison_status("runtime difference", drift)})
    return differences


def comparison_status(category, drift):
    severity = {"regression": "failure", "coverage loss": "coverage loss"}.get(category, "informational")
    return {
        "severity": severity,
        "comparability": "configuration changed" if drift else "no configuration change detected",
        "attribution": ("undetermined across configuration change" if drift else "undetermined")
        if severity != "informational" else None,
    }


def load(path):
    return json.loads(path.read_text(encoding="utf-8"))


def latest_rerun_source(runs_dir):
    for source in sorted(runs_dir.iterdir(), reverse=True):
        try:
            matrix = load(source / "matrix.json")
            metadata = load(source / "sources.json")
            report = load(source / "report/report.json")
            ids = {row["id"] for row in matrix["runtimes"]}
            selected_path = source / "executed-selection.json"
            selected = set(load(selected_path)) if selected_path.is_file() else set()
            observed = {row["runtime"]["id"] for row in report["runs"]}
            if (matrix["schema_version"] == SCHEMA and report["schema_version"] == SCHEMA
                    and re.fullmatch(r"[0-9a-f]{40}", metadata["repo_commit"])
                    and ids and len(ids) == len(matrix["runtimes"])
                    and selected <= ids and observed == (selected or ids)
                    and len(observed) == len(report["runs"])
                    and all("installation" in row for row in matrix["runtimes"])):
                return source
        except (OSError, ValueError, KeyError, TypeError):
            continue
    raise ValueError("no nonempty saved matrix with a complete row report")


def uncertified_runtime_ids(report_path, profile):
    report = json.loads(report_path.read_text(encoding="utf-8"), object_pairs_hook=unique_object)
    result = certification.evaluate(report, report_path, profile, SCHEMA, parse_event)
    return {row["runtime_id"] for row in result["runtimes"] if row["outcome"] != "CERTIFIED"}


def matrix_template(control):
    runtimes = []
    for system in ("linux", "windows"):
        linux = system == "linux"
        for engine, engine_name in (("hlds", "Valve HLDS"), ("rehlds", "ReHLDS")):
            for manager, manager_name in (("classic", "alliedmodders/metamod-hl1"), ("metamod-r", "Metamod-R")):
                games = [("stock", "ricochet", "Stock Ricochet"),
                         ("updated", "ricochet", "ricochet-updated")]
                if control:
                    games.append(("hldm", "valve", "Half-Life Deathmatch"))
                for game, mod, game_name in games:
                    game_binary = {
                        "stock": "ricochet.so" if linux else "mp.dll",
                        "updated": "ricochet_updated.so" if linux else "ricochet_updated.dll",
                        "hldm": "hl.so" if linux else "hl.dll",
                    }[game]
                    runtimes.append({
                        "id": f"{system}-{engine}-{manager}-{game}",
                        "os": system, "installation": f"installations/{system}-{engine}",
                        "mod": mod, "experimental": engine == "hlds" and manager == "metamod-r",
                        "engine": {"implementation": engine_name, "version": "SET_EXACT_VERSION",
                                   "binary": "hlds_linux" if linux else "hlds.exe",
                                   "modules": ["engine_i486.so" if linux else "swds.dll"]},
                        "metamod": {"implementation": manager_name, "version": "SET_EXACT_VERSION",
                                    "binary": f"{mod}/addons/{manager}/dlls/" + ("metamod_i386.so" if linux else "metamod.dll")},
                        "gamedll": {"implementation": game_name, "version": "SET_EXACT_VERSION",
                                    "binary": f"{mod}/dlls/{game_binary}"},
                        "maps": ["crossfire", "boot_camp"] if mod == "valve" else ["rc_arena", "rc_deathmatch"],
                    })
    return {"schema_version": SCHEMA, "runtimes": runtimes}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    template = sub.add_parser("template", help="print a matrix configuration with editable paths and versions")
    template.add_argument("--hldm", action="store_true")
    run = sub.add_parser("run")
    run.add_argument("config", type=Path)
    run.add_argument("output", type=Path)
    run.add_argument("--only", action="append", default=[])
    run.add_argument("--platform", type=str.lower, choices=("linux", "windows"))
    run.add_argument("--build", action="store_true")
    run.add_argument("--prepare-only", action="store_true", help="keep an isolated copy for manual console operation")
    run.add_argument("--client-wait", type=int, choices=range(0, 601), default=0, metavar="SECONDS")
    imp = sub.add_parser("import")
    imp.add_argument("runtime", type=Path, help="independently identified runtime JSON")
    imp.add_argument("output", type=Path)
    imp.add_argument("logs", nargs="+", type=Path)
    imp.add_argument("--manual", type=Path)
    imp.add_argument("--native-fixture", action="store_true", help="import a single native loader transcript")
    imp.add_argument("--require-native-suite", action="store_true", help="require all native loader probes and the audit plugin protocol")
    diff = sub.add_parser("compare")
    diff.add_argument("baseline", type=Path)
    diff.add_argument("candidate", nargs="?", type=Path)
    diff.add_argument("--allowances", type=Path)
    certify = sub.add_parser("certify", help="evaluate a live report against a versioned compatibility profile")
    certify.add_argument("report", type=Path)
    certify.add_argument("profile", choices=certification.PROFILES)
    certify.add_argument("output", type=Path, help="new directory for certificate.json and certificate.md")
    args = parser.parse_args()
    run_id = str(uuid.uuid4())
    try:
        if args.command == "template":
            print(json.dumps(matrix_template(args.hldm), indent=2))
            return 0
        if args.command == "compare":
            allowances = load(args.allowances) if args.allowances else []
            baseline = load(args.baseline)
            changes = (compare(baseline, load(args.candidate), allowances) if args.candidate
                       else compare_runtimes(baseline, allowances))
            print(json.dumps(changes, indent=2))
            return int(any(change.get("severity") in {"failure", "coverage loss"} for change in changes))
        if args.command == "certify":
            report = json.loads(args.report.read_text(encoding="utf-8"), object_pairs_hook=unique_object)
            certificate = certification.evaluate(report, args.report, args.profile, SCHEMA, parse_event)
            args.output.mkdir(parents=True)
            dump(args.output / "certificate.json", certificate)
            (args.output / "certificate.md").write_text(certification.markdown(certificate), encoding="utf-8")
            return int(not certificate["all_certified"])
        if args.command == "run":
            config_path = args.config.resolve(strict=True)
            config = load(config_path)
            if config["schema_version"] != SCHEMA:
                raise ValueError("unsupported configuration schema")
            specs = config["runtimes"]
            ids = [label(spec["id"], "runtime ID") for spec in specs]
            if len(ids) != len(set(ids)) or not set(args.only).issubset(ids):
                raise ValueError("duplicate runtime IDs or unknown --only selection")
            specs = [spec for spec in specs if (not args.only or spec["id"] in args.only)
                     and (not args.platform or spec["os"] == args.platform)]
            if not specs:
                raise ValueError("no runtimes configured")
            output_path = args.output.resolve()
            for spec in specs:
                source = (config_path.parent / spec["installation"]).resolve()
                if output_path.is_relative_to(source):
                    raise ValueError("output must be outside each source installation")
            args.output.mkdir(parents=True)
            runs = []
            for spec in specs:
                output = (args.output / spec["id"]).resolve()
                output.mkdir()
                runs.append(run_one(spec, config_path.parent, output, run_id, args.build, args.client_wait, args.prepare_only))
                finish(args.output, run_id, runs)
            if args.prepare_only:
                return int(any(run["execution"] != "PREPARED" for run in runs))
        else:
            if args.require_native_suite and not args.native_fixture:
                raise ValueError("--require-native-suite requires --native-fixture")
            runtime = load(args.runtime)
            runtime["evidence_scope"] = "native-fixture" if args.native_fixture else "runtime"
            label(runtime["id"], "runtime ID")
            records = manual_results(args.manual)
            args.output.mkdir(parents=True)
            metadata = {"run_id": run_id, "execution": "IMPORTED", "executed": True,
                        "require_native_suite": args.require_native_suite,
                        "exit_status": runtime.get("exit_status"), "original_logs": [str(path) for path in args.logs]}
            runs = [aggregate(args.output.resolve(), runtime, metadata, args.logs, records)]
            finish(args.output, run_id, runs)
        return int(any(run["execution"] not in {"COMPLETE", "IMPORTED"}
                       or ((not run["native_fixture_results"] or run["exit_status"] != 0) if run["evidence_scope"] == "native-fixture"
                           else not run["protocol_complete"] or any(identity["result"] != "PASS"
                                                                     for identity in run["runtime_identity"].values()))
                       or (run.get("require_native_suite") and not run["protocol_complete"])
                       or run["diagnostics"] or run["protocol_warnings"]
                       or run["exit_status"] not in {None, 0}
                       or any(transition["result"] != "PASS" for transition in run.get("map_transitions", []))
                       or any(test.get("result") == "FAIL" or test.get("manual_conflict")
                              for test in run["tests"] + run["contract_findings"])
                       for run in runs))
    except (OSError, ValueError, KeyError, TypeError) as error:
        parser.exit(2, f"audit: {error}\n")


if __name__ == "__main__":
    raise SystemExit(main())
