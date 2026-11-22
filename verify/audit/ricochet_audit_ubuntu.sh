#!/usr/bin/env bash
# Ricochet / metamod-sdk-rs runtime audit lab for Ubuntu x86_64.
# Creates disposable HLDS and ReHLDS installations; real servers are never patched in place.
set -Eeuo pipefail
umask 077
IFS=$'\n\t'

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
WORKDIR="${HOME}/ricochet-audit-lab"
REPO_REF="master"
AUDIT_PLUGIN=""
PLUGIN_PROVENANCE=""
CERTIFY_PROFILE=""
MEMCHECK=0
STEAM_USER=""
UPDATED_DLL=""
VALVE_VERSION=""
REHLDS_CONSOLE_VERSION=""
CLASSIC_CONSOLE_VERSION="1.21.1-am"
METAMODR_CONSOLE_VERSION=""
REHLDS_ZIP=""
METAMODR_ZIP=""
CLASSIC_ZIP=""
MAP_CYCLES=2
CLIENT_WAIT=0
EXPERIMENTAL=0
PREPARE_ONLY=0
WITH_NATIVE=0
WITH_RUST_TESTS=0
SKIP_APT=0
SKIP_STEAM=0
KEEP_OLD=1
RERUN_SOURCE=""
RERUN_RUNNER_CHECKOUT=""
FAILED_ONLY=0
MAP_CYCLES_EXPLICIT=0
ONLY_RUNTIMES=()
RERUN_DISALLOWED=()

usage() {
  cat <<'HELP'
Usage: ./ricochet_audit_ubuntu.sh [options]

  --workdir PATH               Workspace (default ~/ricochet-audit-lab)
  --rerun [latest|RUN_ID]      Reuse an existing run's installed servers and audit plugin;
                               no SteamCMD download, apt install, or server re-provisioning
  --rerun-runner-checkout PATH
                               Explicitly use a clean runner checkout for a saved rerun
  --only RUNTIME_ID           Audit only this matrix row (repeatable)
  --failed-only               With --rerun, retry only rows that failed or were incomplete
  --plugin PATH                Prebuilt Linux i686 metamod-audit .so (required for fresh runs)
  --plugin-provenance PATH     Build provenance JSON for the prebuilt plugin
  --certify PROFILE            Evaluate the live report after the audit
  --memcheck                   Collect separate Valgrind memory-safety v1 reports;
                               requires Valgrind with i386 support on the audit host
  --repo-ref REF               metamod-sdk-rs branch/tag/commit (default master)
  --steam-user NAME            Use Steam account instead of anonymous if necessary.
                               SteamCMD handles password/Steam Guard interactively.
  --updated-gamedll PATH       Your *Linux i686* ricochet-updated .so (optional)
  --valve-version TOKEN        Valve HLDS console 'Exe version' token
  --rehlds-console-version X   Override ReHLDS console version token
  --classic-console-version X  Override classic Metamod console version token
  --metamodr-console-version X Override Metamod-R console version token
  --rehlds-zip PATH            Use a locally supplied official ReHLDS release ZIP
  --metamodr-zip PATH          Use a locally supplied Metamod-R release ZIP
  --classic-zip PATH           Use a locally supplied classic Metamod release ZIP
  --map-cycles N               Repeat map itinerary N times, 1..8 (default 2)
  --client-wait SECONDS        Pause for manual client tests, 0..600 (default 0)
  --experimental              Also attempt HLDS + Metamod-R (upstream unsupported)
  --prepare-only              Stage matrix for manually operated consoles
  --with-native               Also build and run strict C++/Rust i686 loader checks
  --with-rust-tests           Also run Rust workspace and Python runner tests
  --skip-apt                  Do not install Ubuntu packages
  --skip-steam                Reuse already downloaded SteamCMD HLDS files
                               (implicit in --rerun mode)
  -h, --help                  Show this help

Run as your normal sudo-capable user, NOT as root. Ubuntu 22.04/24.04/26.04
x86_64 with network access, disk space, and Steam terms accepted is assumed.
Steam downloads use app 90 / 'mod ricochet' / 'steam_legacy'. No Valve files
are redistributed. All game servers default to loopback-only, LAN mode.

Fresh run with a CI artifact:
  ./ricochet_audit_ubuntu.sh --plugin ./libmetamod_audit.so

The live matrix uses a saved copy of the supplied plugin for every row.
Rust and native-fixture checks are off by default and build separately
from --repo-ref when requested.

Rerun examples:
  ./ricochet_audit_ubuntu.sh --rerun                 # latest saved matrix
  ./ricochet_audit_ubuntu.sh --rerun latest --failed-only
  ./ricochet_audit_ubuntu.sh --rerun 20261120T210000Z-1234 \
    --only linux-rehlds-metamod-r-stock

Reruns create a new runs/<timestamp>-<pid> directory. They reuse the saved
server snapshots, the existing audit plugin, and the pinned SDK checkout.
The source run must remain on disk. The default rerun requires its pinned
runner revision. An explicit --rerun-runner-checkout preserves the saved
artifact provenance and records the selected runner revision separately.
HELP
}

while (($#)); do
  case "$1" in
    --workdir|--plugin|--plugin-provenance|--certify|--repo-ref|--steam-user|--updated-gamedll|--valve-version|--rehlds-console-version|--classic-console-version|--metamodr-console-version|--rehlds-zip|--metamodr-zip|--classic-zip|--map-cycles|--client-wait|--rerun-runner-checkout)
      (($# >= 2)) || { echo "Missing value: $1" >&2; exit 2; }
      key="$1"; val="$2"; shift 2
      case "$key" in
        --plugin|--plugin-provenance|--repo-ref|--steam-user|--updated-gamedll|--valve-version|--rehlds-console-version|--classic-console-version|--metamodr-console-version|--rehlds-zip|--metamodr-zip|--classic-zip)
          RERUN_DISALLOWED+=("$key");;
      esac
      case "$key" in
        --workdir) WORKDIR="$val";; --plugin) AUDIT_PLUGIN="$val";; --repo-ref) REPO_REF="$val";;
        --plugin-provenance) PLUGIN_PROVENANCE="$val";; --certify) CERTIFY_PROFILE="$val";;
        --steam-user) STEAM_USER="$val";; --updated-gamedll) UPDATED_DLL="$val";;
        --valve-version) VALVE_VERSION="$val";;
        --rehlds-console-version) REHLDS_CONSOLE_VERSION="$val";;
        --classic-console-version) CLASSIC_CONSOLE_VERSION="$val";;
        --metamodr-console-version) METAMODR_CONSOLE_VERSION="$val";;
        --rehlds-zip) REHLDS_ZIP="$val";; --metamodr-zip) METAMODR_ZIP="$val";;
        --classic-zip) CLASSIC_ZIP="$val";; --map-cycles) MAP_CYCLES="$val"; MAP_CYCLES_EXPLICIT=1;;
        --client-wait) CLIENT_WAIT="$val";;
        --rerun-runner-checkout) RERUN_RUNNER_CHECKOUT="$val";;
      esac;;
    --rerun)
      RERUN_SOURCE=latest
      if (($# >= 2)) && [[ $2 != -* ]]; then RERUN_SOURCE="$2"; shift 2; else shift; fi;;
    --only)
      (($# >= 2)) || { echo "Missing value: --only" >&2; exit 2; }
      ONLY_RUNTIMES+=("$2"); shift 2;;
    --failed-only) FAILED_ONLY=1; shift;;
    --experimental) EXPERIMENTAL=1; RERUN_DISALLOWED+=(--experimental); shift;;
    --prepare-only) PREPARE_ONLY=1; shift;;
    --memcheck) MEMCHECK=1; shift;;
    --with-native) WITH_NATIVE=1; shift;;
    --with-rust-tests) WITH_RUST_TESTS=1; shift;;
    --skip-apt) SKIP_APT=1; shift;;
    --skip-steam) SKIP_STEAM=1; shift;;
    -h|--help) usage; exit 0;;
    *) echo "Unknown option: $1" >&2; usage >&2; exit 2;;
  esac
done

[[ $EUID -ne 0 ]] || { echo "Do not run SteamCMD or the servers as root. Run as a normal sudo user." >&2; exit 2; }
[[ $(uname -s) == Linux && $(uname -m) == x86_64 ]] || { echo "This script requires x86_64 Linux." >&2; exit 2; }
[[ -r /etc/os-release ]] || { echo "Cannot identify Ubuntu." >&2; exit 2; }
# shellcheck source=/dev/null
source /etc/os-release
[[ ${ID:-} == ubuntu ]] || { echo "This installer currently supports Ubuntu only (found ${ID:-unknown})." >&2; exit 2; }
[[ $MAP_CYCLES =~ ^[1-8]$ ]] || { echo "--map-cycles must be 1..8" >&2; exit 2; }
[[ $CLIENT_WAIT =~ ^[0-9]+$ ]] && ((CLIENT_WAIT <= 600)) || { echo "--client-wait must be 0..600" >&2; exit 2; }
[[ $REPO_REF =~ ^[A-Za-z0-9._/-]{1,128}$ ]] || { echo "Invalid --repo-ref" >&2; exit 2; }
(( ! FAILED_ONLY )) || [[ -n $RERUN_SOURCE ]] || { echo "--failed-only requires --rerun" >&2; exit 2; }
if [[ -z $RERUN_SOURCE ]]; then
  [[ -n $AUDIT_PLUGIN && -f $AUDIT_PLUGIN && -r $AUDIT_PLUGIN ]] || {
    echo "Fresh runs require --plugin PATH to a readable prebuilt metamod-audit .so." >&2
    exit 2
  }
fi
for selected in "${ONLY_RUNTIMES[@]}"; do
  [[ $selected =~ ^[A-Za-z0-9_-]{1,64}$ ]] || { echo "Invalid --only runtime ID: $selected" >&2; exit 2; }
done

say() { printf '\n[%s] %s\n' "$(date -u +%H:%M:%S)" "$*" >&2; }
die() { echo "ERROR: $*" >&2; exit 1; }
validate_elf32() {
  if ! python3 "$SCRIPT_DIR/elf.py" "$1"; then
    file -- "$1" >&2 || true
    return 1
  fi
}
trap 'echo "ERROR near line $LINENO. Existing files and partial logs are preserved." >&2' ERR
[[ -z $CERTIFY_PROFILE ]] || (( ! PREPARE_ONLY )) || die "--certify requires a live run"
(( ! MEMCHECK || ! PREPARE_ONLY )) || die "--memcheck requires a live run"
[[ -z $RERUN_RUNNER_CHECKOUT || -n $RERUN_SOURCE ]] || die "--rerun-runner-checkout requires --rerun"
[[ -z $PLUGIN_PROVENANCE || -r $PLUGIN_PROVENANCE ]] || die "Cannot read --plugin-provenance"

# A single validation path is used by initial provisioning and by reruns.
run_native_suite() {
if (( WITH_NATIVE )); then
  say "Running Linux i686 ABI/loader checks with strict native-fixture evidence validation."
  # The repository's collector retains the loader's real exit status, captures
  # its transcript even on failure, and imports it with BOTH --native-fixture
  # and --require-native-suite. Missing/duplicate probes, shutdown-order
  # errors, sanitizer diagnostics, and a failed loader must fail this step.
  # Do not manufacture an exit_status of 0 or accept partial native evidence.
  [[ -f "$REPO/verify/audit/collect-native.sh" ]] || \
    die "Selected metamod-sdk-rs revision lacks verify/audit/collect-native.sh; use commit 8f2673f or newer."
  grep -q -- '--native-fixture' "$REPO/verify/audit/collect-native.sh" && \
    grep -q -- '--require-native-suite' "$REPO/verify/audit/collect-native.sh" || \
    die "Selected metamod-sdk-rs revision does not enforce the complete native suite; use commit 8f2673f or newer."
  grep -q -- '--require-native-suite' "$REPO/verify/audit/runner.py" || \
    die "Selected metamod-sdk-rs revision lacks strict native-suite import support."
  ( cd "$REPO" && bash verify/audit/collect-native.sh \
      "$RUNROOT/native-fixture" linux-i686-native-loader )
  # Check the collector's structured output, not just its process exit code.
  # NATIVE_SUITE is imported from the pinned repository instead of duplicating
  # the required-probe list in this bootstrap script.
  python3 - "$REPO/verify/audit" "$RUNROOT/native-fixture/report/report.json" <<'PY_NATIVE'
import json, pathlib, sys
sys.path.insert(0, sys.argv[1])
from runner import NATIVE_SUITE
path = pathlib.Path(sys.argv[2])
report = json.loads(path.read_text(encoding='utf-8'))
runs = report.get('runs', [])
if len(runs) != 1:
    raise SystemExit('Strict native report must contain exactly one run')
run = runs[0]
items = run.get('native_fixture_results', [])
ids = [x.get('test') for x in items]
if (run.get('evidence_scope') != 'native-fixture'
        or run.get('require_native_suite') is not True
        or run.get('protocol_complete') is not True
        or run.get('exit_status') != 0
        or run.get('diagnostics') or run.get('protocol_warnings')
        or len(ids) != len(set(ids)) or set(ids) != NATIVE_SUITE
        or any(item.get('result') != 'PASS' for item in items)):
    raise SystemExit('Strict native audit was incomplete or failed; inspect ' + str(path))
print(f'Verified all {len(NATIVE_SUITE)} required native probes with complete shutdown evidence.')
PY_NATIVE
fi
}

run_workspace_tests() {
if (( WITH_RUST_TESTS )); then
  say "Running Rust workspace tests and Python audit-runner tests."
  ( cd "$REPO" && cargo test --workspace --locked )
  ( cd "$REPO" && python3 -m unittest discover -s verify/audit -v )
fi
}

check_memcheck_support() {
  if (( MEMCHECK )); then
    local help
    help="$(python3 "$REPO/verify/audit/runner.py" run --help)" || die "Cannot inspect runner capabilities"
    [[ $help == *--memcheck* ]] || die "This runner predates --memcheck. Select a newer clean checkout with --rerun-runner-checkout PATH."
  fi
}

run_matrix_and_summary() {
check_memcheck_support
say "Running configured actual HLDS/ReHLDS audit matrix (${#ONLY_RUNTIMES[@]} explicit runtime filters)."
RUNNER=(python3 "$REPO/verify/audit/runner.py" run "$RUNROOT/matrix.json" "$RUNROOT/report" --platform linux)
((CLIENT_WAIT == 0)) || RUNNER+=(--client-wait "$CLIENT_WAIT")
((PREPARE_ONLY == 0)) || RUNNER+=(--prepare-only)
((MEMCHECK == 0)) || RUNNER+=(--memcheck)
for selected in "${ONLY_RUNTIMES[@]}"; do RUNNER+=(--only "$selected"); done
set +e
"${RUNNER[@]}"
STATUS=$?
set -e
python3 - "$RUNROOT/executed-selection.json" "${ONLY_RUNTIMES[@]}" <<'PY_SELECTION'
import json, pathlib, sys
pathlib.Path(sys.argv[1]).write_text(json.dumps(sys.argv[2:]), encoding='utf-8')
PY_SELECTION

# Generate an operator-friendly evidence summary even if some real runtimes
# failed. Never promote missing, skipped, or inconclusive probes to PASS.
python3 - "$RUNROOT" "$AUDIT_COMMIT" "$WITH_NATIVE" <<'PY_SUMMARY'
import json, pathlib, sys
root = pathlib.Path(sys.argv[1])
commit = sys.argv[2]
skipped_native = sys.argv[3] == '0'
output = root / 'audit-summary.md'
report_file = root / 'report/report.json'
native_file = root / 'native-fixture/report/report.json'

lines = [
    '# Ricochet / metamod-sdk-rs audit evidence summary', '',
    f'- Audit runner checkout commit: `{commit}`',
    f'- Matrix: `{root / "matrix.json"}`',
    f'- Detailed runtime report: `{report_file}`', '',
    'A map-transition PASS establishes that the expected lifecycle callbacks were',
    'observed. It does **not** by itself establish rejection of old entity handles.',
    'Stale-handle checks are reported separately, per observed map transition.', ''
]

def status_word(values):
    if not values:
        return 'UNOBSERVED'
    if 'FAIL' in values:
        return 'FAIL'
    if all(v == 'PASS' for v in values):
        return 'PASS'
    return 'INCONCLUSIVE'

def text(value):
    return str(value).replace('|', r'\|').replace('\n', ' ')

lines.append('## Native C++/Rust fixture')
if skipped_native:
    lines.append('**NOT REQUESTED (enable with `--with-native`)**; not certified.')
elif native_file.exists():
    data = json.loads(native_file.read_text(encoding='utf-8'))
    runs = data.get('runs', [])
    if len(runs) != 1:
        lines.append('**INCOMPLETE**: native report does not contain exactly one run.')
    else:
        native = runs[0]
        probes = native.get('native_fixture_results', [])
        complete = (native.get('protocol_complete') is True
                    and native.get('exit_status') == 0
                    and native.get('require_native_suite') is True
                    and len(probes) == 6
                    and all(p.get('result') == 'PASS' for p in probes))
        lines.append(f'**{"PASS" if complete else "NOT VERIFIED"}**: '
                     f'{len(probes)} structured native probes; '
                     f'ordered shutdown protocol={native.get("protocol_complete")}; '
                     f'exit status={native.get("exit_status")}.')
else:
    lines.append('**MISSING**: no native fixture report was produced.')

lines += ['', '## Live server matrix', '']
if not report_file.exists():
    lines += ['**No real-runtime report was produced.** This does not imply success.', '']
else:
    data = json.loads(report_file.read_text(encoding='utf-8'))
    runs = data.get('runs', [])
    lines += [
        '| Runtime | Execution | Identities | Protocol | Map transitions | Stale handle checks | Failed probes |',
        '| --- | --- | --- | --- | --- | --- | --- |',
    ]
    for run in runs:
        ident = run.get('runtime_identity') or {}
        identity_statuses = [x.get('result', 'INCONCLUSIVE') for x in ident.values()]
        identities = (status_word(identity_statuses) if identity_statuses else 'UNVERIFIED')
        transitions = run.get('map_transitions', [])
        transitions_status = status_word([t.get('result', 'INCONCLUSIVE') for t in transitions])
        checks = [v for t in transitions for v in t.get('previous_handle_checks', [])]
        stale_status = status_word(checks)
        failed = [t.get('test') for t in run.get('tests', []) if t.get('result') == 'FAIL']
        lines.append('| ' + ' | '.join(map(text, (
            run.get('runtime', {}).get('id', 'unknown'),
            run.get('execution', 'UNKNOWN'), identities,
            'COMPLETE' if run.get('protocol_complete') else 'INCOMPLETE',
            transitions_status, stale_status,
            ', '.join(failed) if failed else 'None observed',
        ))) + ' |')
    for run in runs:
        name = run.get('runtime', {}).get('id', 'unknown')
        lines += ['', f'### `{name}`: map-by-map lifecycle evidence', '']
        transitions = run.get('map_transitions', [])
        if not transitions:
            lines += ['No map transitions recorded; no per-transition stale-handle claim is possible.', '']
            continue
        lines += [
            '| Visit | Requested map | Transition callback | Stale-handle check | Callback sequences |',
            '| --- | --- | --- | --- | --- |',
        ]
        for t in transitions:
            lines.append('| ' + ' | '.join(map(text, (
                t.get('index', '?'), t.get('requested_map', '?'),
                t.get('result', 'INCONCLUSIVE'),
                status_word(t.get('previous_handle_checks', [])),
                ', '.join(map(str, t.get('callback_sequences', []))) or 'UNOBSERVED',
            ))) + ' |')
        lines.append('')
    lines += [
        'Aggregate stale-handle probe results are available separately in',
        '`report.json` (`tests[]`, identifier `map.stale.v1`). A PASS in one',
        'transition must not be interpreted as coverage of every transition.', ''
    ]
output.write_text('\n'.join(lines).rstrip() + '\n', encoding='utf-8')
print(f'Wrote evidence summary: {output}')
PY_SUMMARY

say "Finished: audit exit status $STATUS"
CERTIFY_STATUS=0
if [[ -n $CERTIFY_PROFILE ]]; then
  if python3 "$REPO/verify/audit/runner.py" certify "$RUNROOT/report/report.json" \
    "$CERTIFY_PROFILE" "$RUNROOT/certification"; then
    say "Certification passed: $RUNROOT/certification/certificate.md"
  else
    CERTIFY_STATUS=$?
    say "Certification exited with status $CERTIFY_STATUS. Inspect $RUNROOT/certification."
  fi
fi
echo "Setup and audit files: $RUNROOT"
echo "Matrix: $RUNROOT/matrix.json"
echo "Report: $RUNROOT/report/report.md"
echo "Evidence summary: $RUNROOT/audit-summary.md"
if ((MEMCHECK)) && [[ -f $RUNROOT/report/memory-safety.md ]]; then
  echo "Memory-safety evidence: $RUNROOT/report/memory-safety.md"
fi
echo "Audit completion is separate from certification. Use --certify to evaluate a profile."
if ((STATUS != 0)); then
  echo "Some configurations did not pass; inspect the reports and per-runtime logs."
  if (( ! PREPARE_ONLY && ! MEMCHECK )) && [[ -t 0 ]]; then
    echo "Some HLDS versions do not process stdin through a pipe."
    read -r -p 'Also create manually launchable isolated copies? [y/N]: ' manual_answer
    if [[ $manual_answer =~ ^[Yy]([Ee][Ss])?$ ]]; then
      say "Preparing individual servers for manual console auditing."
      python3 "$REPO/verify/audit/runner.py" run "$RUNROOT/matrix.json" \
        "$RUNROOT/manual" --platform linux --prepare-only || true
      echo "Manual server commands and working directories are in $RUNROOT/manual/*/launch.json"
    fi
  fi
fi
exit "$((STATUS != 0 ? STATUS : CERTIFY_STATUS))"
}

# --rerun is deliberately reached before apt, SteamCMD, git fetch, release
# downloads, or installation staging. It never mutates the saved install trees.
if [[ -n $RERUN_SOURCE ]]; then
  ((${#RERUN_DISALLOWED[@]} == 0)) || die "Provisioning flags cannot be used with --rerun (${RERUN_DISALLOWED[*]}); create a new installation to change these."

  [[ -d $WORKDIR ]] || die "Workspace does not exist: $WORKDIR"
  WORKDIR="$(cd "$WORKDIR" && pwd -P)"
  command -v python3 >/dev/null || die "Python 3 is required for reruns"
  command -v git >/dev/null || die "Git is required for reruns"
  REPO="${RERUN_RUNNER_CHECKOUT:-$WORKDIR/metamod-sdk-rs}"
  REPO="$(cd -- "$REPO" && pwd -P)" || die "Missing source checkout"
  [[ -e $REPO/.git ]] || die "Missing source checkout: $REPO"
  if [[ -n $RERUN_RUNNER_CHECKOUT ]]; then
    checkout_status="$(git -C "$REPO" status --porcelain)" || die "Cannot inspect checkout: $REPO"
    [[ -z $checkout_status ]] || die "Explicit runner checkout must be clean: $REPO"
  fi
  if [[ $RERUN_SOURCE == latest ]]; then
    RERUN_ORIGIN="$(python3 - "$REPO/verify/audit" "$WORKDIR/runs" <<'PY_LATEST'
import pathlib, sys
sys.path.insert(0, sys.argv[1])
from runner import latest_rerun_source
print(latest_rerun_source(pathlib.Path(sys.argv[2])))
PY_LATEST
)" || die "No completed saved run found under $WORKDIR/runs"
  else
    [[ $RERUN_SOURCE =~ ^[A-Za-z0-9_-]{1,128}$ ]] || die "--rerun expects 'latest' or a run ID beneath $WORKDIR/runs"
    RERUN_ORIGIN="$WORKDIR/runs/$RERUN_SOURCE"
  fi
  [[ -f "$RERUN_ORIGIN/matrix.json" ]] || die "No saved runtime matrix: $RERUN_ORIGIN/matrix.json"
  [[ -f "$RERUN_ORIGIN/sources.json" ]] || die "No source metadata: $RERUN_ORIGIN/sources.json"
  RERUN_ORIGIN="$(cd "$RERUN_ORIGIN" && pwd -P)"
  AUDIT_COMMIT="$(python3 - "$RERUN_ORIGIN/sources.json" <<'PY_COMMIT'
import json,sys
print(json.load(open(sys.argv[1], encoding='utf-8')).get('repo_commit', ''))
PY_COMMIT
)"
  [[ $AUDIT_COMMIT =~ ^[0-9a-f]{40}$ ]] || die "Saved source metadata lacks a valid pinned Git commit"
  ACTUAL_COMMIT="$(git -C "$REPO" rev-parse HEAD)"
  [[ -n $RERUN_RUNNER_CHECKOUT || $AUDIT_COMMIT == "$ACTUAL_COMMIT" ]] || die "SDK checkout moved: expected $AUDIT_COMMIT, got $ACTUAL_COMMIT. Restore the pinned commit or use --rerun-runner-checkout PATH."
  [[ -f "$REPO/verify/audit/runner.py" ]] || die "Missing audit runner in $REPO"
  check_memcheck_support
  AUDIT_COMMIT="$ACTUAL_COMMIT"
  RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)-$$"
  RUNROOT="$WORKDIR/runs/$RUN_ID"
  [[ ! -e $RUNROOT ]] || die "Output run directory already exists: $RUNROOT"
  mkdir -p "$RUNROOT"
  say "Rerunning from $RERUN_ORIGIN into $RUNROOT without reinstalling anything."
  # Resolve source installations and plugin against the old matrix directory,
  # so this new run does not require another multi-GB copy or patch an old server.
  # Resolve the --failed-only filter from the PREVIOUS run's report, not a
  # pass/fail heuristic applied to a partially written new report.
  SELECTED_FILE="$RUNROOT/selected-runtimes.txt"
  python3 - "$RERUN_ORIGIN" "$RUNROOT" "$FAILED_ONLY" "$MAP_CYCLES_EXPLICIT" "$MAP_CYCLES" "$SELECTED_FILE" "$CERTIFY_PROFILE" "$REPO/verify/audit" "$AUDIT_COMMIT" "${ONLY_RUNTIMES[@]}" <<'PY_RERUN'
import json, pathlib, sys
source, dest = map(pathlib.Path, sys.argv[1:3])
failed_only, override_cycles, cycles = map(int, sys.argv[3:6])
selected_file = pathlib.Path(sys.argv[6])
profile = sys.argv[7]
sys.path.insert(0, sys.argv[8])
runner_commit = sys.argv[9]
requested = set(sys.argv[10:])
matrix = json.loads((source / 'matrix.json').read_text(encoding='utf-8'))
if matrix.get('schema_version') != 1 or not isinstance(matrix.get('runtimes'), list):
    raise SystemExit('saved runtime matrix is invalid')
all_ids = [r['id'] for r in matrix['runtimes']]
if len(set(all_ids)) != len(all_ids) or (requested - set(all_ids)):
    raise SystemExit(f'unknown or duplicate runtime ID: {sorted(requested - set(all_ids))}')
failed = set()
if failed_only:
    old_report = source / 'report' / 'report.json'
    if not old_report.is_file():
        raise SystemExit('--failed-only needs an existing report/report.json for the source run')
    data = json.loads(old_report.read_text(encoding='utf-8'))
    executed = {r.get('runtime', {}).get('id'): r for r in data.get('runs', [])}
    if profile:
        from runner import uncertified_runtime_ids
        failed = uncertified_runtime_ids(old_report, profile) | (set(all_ids) - executed.keys())
    for rid in all_ids if not profile else ():
        record = executed.get(rid)
        if record is None:
            failed.add(rid)
            continue
        identities = record.get('runtime_identity') or {}
        unhealthy = (record.get('execution') != 'COMPLETE'
                     or record.get('protocol_complete') is not True
                     or record.get('exit_status') != 0
                     or bool(record.get('diagnostics') or record.get('protocol_warnings')
                             or record.get('capture_errors') or record.get('truncated_streams'))
                     or not identities or any(x.get('result') != 'PASS' for x in identities.values())
                     or any(x.get('result') != 'PASS' for x in record.get('map_transitions', []))
                     or any(x.get('result') == 'FAIL' or x.get('manual_conflict')
                            for x in record.get('tests', [])))
        if unhealthy:
            failed.add(rid)
rows = []
for r in matrix['runtimes']:
    if requested and r['id'] not in requested:
        continue
    if failed_only and r['id'] not in failed:
        continue
    new = dict(r)
    installation = pathlib.Path(new['installation'])
    if not installation.is_absolute():
        installation = source / installation
    installation = installation.resolve(strict=True)
    if not installation.is_dir():
        raise SystemExit(f'saved installation unavailable: {installation}')
    new['installation'] = str(installation)
    if 'plugin' in new:
        plugin = pathlib.Path(new['plugin'])
        if not plugin.is_absolute():
            plugin = source / plugin
        plugin = plugin.resolve(strict=True)
        if not plugin.is_file():
            raise SystemExit(f'audit plugin unavailable: {plugin}')
        new['plugin'] = str(plugin)
    if override_cycles:
        new['map_cycles'] = cycles
    rows.append(new)
(dest / 'matrix.json').write_text(json.dumps({'schema_version':1, 'runtimes':rows}, indent=2) + '\n', encoding='utf-8')
selected_file.write_text('\n'.join(r['id'] for r in rows) + ('\n' if rows else ''), encoding='utf-8')
source_meta = json.loads((source / 'sources.json').read_text(encoding='utf-8'))
source_meta['source_repo_commit'] = source_meta['repo_commit']
source_meta['repo_commit'] = runner_commit
source_meta['rerun_of'] = str(source)
source_meta['rerun_selection'] = {'failed_only':bool(failed_only), 'requested':sorted(requested), 'certification_profile':profile or None,
                                  'map_cycles_override':cycles if override_cycles else None}
(dest / 'sources.json').write_text(json.dumps(source_meta, indent=2) + '\n', encoding='utf-8')
print(f'Selected {len(rows)}/{len(all_ids)} runtime configurations for rerun:')
for r in rows: print(' - ' + r['id'])
PY_RERUN
  # The selection is encoded in the new matrix. Do not pass an out-of-date
  # --only list to the runner, because it validates filters against that matrix.
  ONLY_RUNTIMES=()
  if [[ ! -s $SELECTED_FILE ]]; then
    say "No runtime rows match the rerun selection; nothing to execute."
    echo "Selection: $SELECTED_FILE"
    exit 0
  fi
  if (( WITH_NATIVE || WITH_RUST_TESTS )); then
    command -v cargo >/dev/null || [[ -x "$HOME/.cargo/bin/cargo" ]] || die "Cargo is required for the requested rerun checks"
    export PATH="$HOME/.cargo/bin:$PATH"
  fi
  run_workspace_tests
  run_native_suite
  run_matrix_and_summary
fi

if (( ! SKIP_APT )); then
  say "Installing Ubuntu prerequisites (sudo may prompt)."
  build_packages=()
  if (( WITH_NATIVE || WITH_RUST_TESTS )); then
    build_packages=(build-essential gcc-multilib g++-multilib clang libclang-dev pkg-config)
  fi
  command -v sudo >/dev/null || die "sudo is required for package installation. Install sudo and give this user sudo access, or rerun with --skip-apt after provisioning packages."
  sudo dpkg --add-architecture i386
  sudo apt-get update
  sudo DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    ca-certificates curl git python3 python3-pip unzip zip tar gzip rsync file \
    "${build_packages[@]}" \
    lib32gcc-s1 lib32stdc++6 libc6-i386 libstdc++6:i386 libgcc-s1:i386 \
    zlib1g:i386 libsdl2-2.0-0:i386 gnupg coreutils findutils binutils procps
fi
for cmd in curl git python3 unzip rsync file timeout sha256sum; do
  command -v "$cmd" >/dev/null || die "Missing prerequisite: $cmd (remove --skip-apt or install it)."
done

mkdir -p "$WORKDIR"
WORKDIR="$(cd "$WORKDIR" && pwd -P)"
STEAMCMD_DIR="$WORKDIR/steamcmd"
BASE="$WORKDIR/steam-legacy"
REPO="$WORKDIR/metamod-sdk-rs"
DOWNLOADS="$WORKDIR/downloads"
RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)-$$"
RUNROOT="$WORKDIR/runs/$RUN_ID"
mkdir -p "$STEAMCMD_DIR" "$BASE" "$DOWNLOADS" "$RUNROOT"
PLUGIN="$RUNROOT/libmetamod_audit.so"
cp -- "$AUDIT_PLUGIN" "$PLUGIN"
validate_elf32 "$PLUGIN" || die "Invalid audit plugin."
PLUGIN_SHA="$(sha256sum "$PLUGIN" | cut -d' ' -f1)"


if [[ ! -x "$STEAMCMD_DIR/steamcmd.sh" ]]; then
  say "Installing official Valve SteamCMD (no password stored by this script)."
  curl --proto '=https' --tlsv1.2 --fail --location --retry 3 \
    https://steamcdn-a.akamaihd.net/client/installer/steamcmd_linux.tar.gz \
    --output "$DOWNLOADS/steamcmd_linux.tar.gz"
  tar -xzf "$DOWNLOADS/steamcmd_linux.tar.gz" -C "$STEAMCMD_DIR"
  chmod +x "$STEAMCMD_DIR/steamcmd.sh"
fi
# GoldSrc locates this library in ~/.steam/sdk32 on some installations.
# SteamCMD may create linux32/steamclient.so only on its first full update.
ensure_steamclient() {
  if [[ -f "$STEAMCMD_DIR/linux32/steamclient.so" ]]; then
    mkdir -p "$HOME/.steam/sdk32"
    if [[ ! -e "$HOME/.steam/sdk32/steamclient.so" && ! -L "$HOME/.steam/sdk32/steamclient.so" ]]; then
      ln -s "$STEAMCMD_DIR/linux32/steamclient.so" "$HOME/.steam/sdk32/steamclient.so"
    fi
  fi
}
ensure_steamclient

# Parse the stock Linux GameDLL path and map list; do not assume Ricochet's
# particular binary or map naming across Steam depots and branches.
inspect_stock() {
  python3 - "$BASE" <<'PY'
import pathlib, re, sys
root = pathlib.Path(sys.argv[1])
mod = root / 'ricochet'
liblist = mod / 'liblist.gam'
if not liblist.is_file():
    raise SystemExit(2)
text = liblist.read_text(encoding='latin-1')
entry = re.findall(r'^\s*gamedll_linux\s+["\']?([^"\'\s]+)', text, flags=re.I | re.M)
if len(entry) != 1:
    raise SystemExit(3)
path = pathlib.PurePosixPath(entry[0].replace('\\','/'))
if path.is_absolute() or '..' in path.parts or not (mod / path).is_file():
    raise SystemExit(4)
maps = sorted(p.stem for p in (mod / 'maps').glob('*.bsp') if re.fullmatch(r'[A-Za-z0-9_-]{1,64}', p.stem))
if len(maps) < 2:
    raise SystemExit(5)
if not (root / 'hlds_linux').is_file() or not (root / 'engine_i486.so').is_file():
    raise SystemExit(6)
print(path)
for name in maps:
    print(name)
PY
}

steam_attempt() {
  local login="$1"
  # The username is passed without a password. SteamCMD itself prompts for
  # password/Steam Guard if this account requires it.
  "$STEAMCMD_DIR/steamcmd.sh" +force_install_dir "$BASE" +login "$login" \
    +app_set_config 90 mod ricochet +app_update 90 -beta steam_legacy validate +quit
}
if (( ! SKIP_STEAM )); then
  say "Downloading HLDS app 90, Ricochet mod, steam_legacy branch."
  logged_in="${STEAM_USER:-anonymous}"
  installed=0
  for attempt in 1 2 3 4; do
    say "SteamCMD validation attempt $attempt/4 ($logged_in)."
    if steam_attempt "$logged_in" && inspect_stock > "$RUNROOT/stock-discovery.txt"; then
      installed=1; break
    fi
    sleep 3
  done
  if (( ! installed )) && [[ $logged_in == anonymous ]] && [[ -t 0 ]]; then
    printf '\nAnonymous download was unsuccessful or incomplete.\n'
    read -r -p 'Steam username for interactive SteamCMD login (Enter to abort): ' logged_in
    [[ -n $logged_in ]] || die "Ricochet app 90 assets are incomplete."
    for attempt in 1 2 3 4; do
      say "Authenticated SteamCMD attempt $attempt/4 (SteamCMD may request credentials/Steam Guard)."
      if steam_attempt "$logged_in" && inspect_stock > "$RUNROOT/stock-discovery.txt"; then
        installed=1; break
      fi
      sleep 3
    done
  fi
  (( installed )) || die "SteamCMD did not produce a complete Ricochet server. Check $BASE/ricochet and Steam entitlement/depot availability."
else
  inspect_stock > "$RUNROOT/stock-discovery.txt" || die "--skip-steam requested, but $BASE is missing valid stock assets."
fi
ensure_steamclient
mapfile -t DISCOVERED < "$RUNROOT/stock-discovery.txt"
STOCK_REL="${DISCOVERED[0]}"
# Prefer known arena maps if present, but rely only on available map files.
MAP_1="${DISCOVERED[1]}"; MAP_2="${DISCOVERED[2]}"
for name in rc_arena rc_deathmatch; do
  for found in "${DISCOVERED[@]:1}"; do
    if [[ $found == "$name" ]]; then
      if [[ $name == rc_arena ]]; then MAP_1="$name"; else MAP_2="$name"; fi
    fi
  done
done
[[ $MAP_1 != "$MAP_2" ]] || { MAP_1="${DISCOVERED[1]}"; MAP_2="${DISCOVERED[2]}"; }
say "Stock GameDLL: ricochet/$STOCK_REL; maps: $MAP_1, $MAP_2"

say "Fetching the metamod-sdk-rs source and pinned upstream submodules."
if [[ ! -d "$REPO/.git" ]]; then
  git clone --recurse-submodules https://github.com/MiguelX413/metamod-sdk-rs.git "$REPO"
fi
(
  cd "$REPO"
  git fetch origin "$REPO_REF" --depth=1
  git checkout --detach FETCH_HEAD
  git submodule update --init --recursive
)
AUDIT_COMMIT="$(git -C "$REPO" rev-parse HEAD)"

if (( WITH_NATIVE || WITH_RUST_TESTS )); then
  if ! command -v rustup >/dev/null || ! command -v cargo >/dev/null; then
    say "Installing Rust through rustup into this user's home directory."
    curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal
  fi
  export PATH="$HOME/.cargo/bin:$PATH"
  command -v rustup >/dev/null || die "rustup not found after installation"
  rustup toolchain install stable --profile minimal
  rustup default stable
  rustup target add i686-unknown-linux-gnu
  if [[ -z ${LIBCLANG_PATH:-} ]]; then
    LIBCLANG_FILE="$(find /usr/lib -name 'libclang.so' -print -quit 2>/dev/null || true)"
    if [[ -n $LIBCLANG_FILE ]]; then export LIBCLANG_PATH="$(dirname "$LIBCLANG_FILE")"; fi
  fi
fi
run_workspace_tests
run_native_suite

# Resolve a release from the public GitHub API; asset file names are selected
# from the response rather than manually inventing a release URL.
release_url() {
  local project="$1" prefix="$2"
  python3 - "$project" "$prefix" <<'PY'
import json, re, sys, urllib.request
project, prefix = sys.argv[1:]
url = f'https://api.github.com/repos/{project}/releases/latest'
req = urllib.request.Request(url, headers={'User-Agent':'Ricochet-Audit-Installer/1.0', 'Accept':'application/vnd.github+json'})
with urllib.request.urlopen(req, timeout=25) as response:
    data = json.load(response)
assets = [a for a in data['assets'] if re.fullmatch(re.escape(prefix) + r'.+\.zip', a['name'])]
if len(assets) != 1:
    raise SystemExit(f'expected exactly one ZIP asset beginning with {prefix!r}: {[(a["name"]) for a in assets]}')
print(data['tag_name'])
print(assets[0]['browser_download_url'])
PY
}
fetch_release() {
  local project="$1" prefix="$2" file="$3" supplied="$4"
  local version url
  if [[ -n $supplied ]]; then
    [[ -f $supplied ]] || die "Local archive does not exist: $supplied"
    cp -- "$supplied" "$DOWNLOADS/$file"
    version="operator-supplied"
  else
    mapfile -t info < <(release_url "$project" "$prefix")
    ((${#info[@]} == 2)) || die "Could not identify $project release; supply an archive via CLI."
    version="${info[0]}"; url="${info[1]}"
    say "Downloading $project release $version"
    curl --proto '=https' --tlsv1.2 --fail --location --retry 3 --retry-all-errors \
      "$url" -o "$DOWNLOADS/$file"
  fi
  [[ -s "$DOWNLOADS/$file" ]] || die "Missing release ZIP for $project"
  printf '%s' "$version"
}

say "Obtaining ReHLDS, Metamod-R, and classic AlliedModders Metamod binaries."
REHLDS_VER="$(fetch_release rehlds/ReHLDS rehlds-bin- rehlds.zip "$REHLDS_ZIP")"
METAMODR_VER="$(fetch_release rehlds/Metamod-R metamod-bin- metamod-r.zip "$METAMODR_ZIP")"
if [[ -n $CLASSIC_ZIP ]]; then
  cp -- "$CLASSIC_ZIP" "$DOWNLOADS/metamod-classic.zip"
  CLASSIC_VER="${CLASSIC_CONSOLE_VERSION}"
else
  CLASSIC_VER="1.21.1-am"
  curl --proto '=https' --tlsv1.2 --fail --location --retry 3 --retry-all-errors \
    https://www.amxmodx.org/release/metamod-1.21.1-am.zip \
    -o "$DOWNLOADS/metamod-classic.zip"
fi
[[ -n $METAMODR_CONSOLE_VERSION ]] || METAMODR_CONSOLE_VERSION="$METAMODR_VER"

# Extract exactly one desired ELF32 binary from ZIP. No arbitrary archive
# files are extracted and archive paths cannot escape the destination.
extract_elf32() {
  local archive="$1" basename_regex="$2" target="$3"
  python3 - "$SCRIPT_DIR" "$archive" "$basename_regex" "$target" <<'PY'
import pathlib, re, sys, zipfile
sys.path.insert(0, sys.argv[1])
from elf import validate_shared_object

archive, pattern, target = sys.argv[2:]
with zipfile.ZipFile(archive) as z:
    choices = [x for x in z.infolist() if not x.is_dir() and re.fullmatch(pattern, pathlib.PurePosixPath(x.filename).name)]
    # Some release archives ship Linux binaries in several arch-specific directories.
    if len(choices) > 1:
        linux32 = [x for x in choices if re.search(r'linux(?:32|_i386|_i686)?', x.filename, re.I)]
        if len(linux32) == 1:
            choices = linux32
    if len(choices) != 1:
        raise SystemExit(f'Ambiguous/missing Linux library {pattern} in {archive}: {[x.filename for x in choices]}')
    data = z.read(choices[0])
    try:
        validate_shared_object(data, f'{archive}:{choices[0].filename}')
    except ValueError as error:
        raise SystemExit(str(error))
    dest = pathlib.Path(target)
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.write_bytes(data)
    dest.chmod(0o755)
    print(f'Extracted {choices[0].filename} -> {dest}')
PY
}

INSTALLS="$RUNROOT/installs"
mkdir -p "$INSTALLS"
say "Creating standalone, separate HLDS and ReHLDS snapshots."
# -L materializes any SteamCMD-created symlinks: the repo runner explicitly
# refuses symlink/reparse-point source trees for safe staging.
stage_installation() {
  local source="$1" destination="$2" sdl="$3" repair dynamic
  local -a excludes=()
  repair="$(python3 - "$source" <<'PY_LINKS'
import os
from pathlib import Path
import subprocess
import sys

root = Path(sys.argv[1])
try:
    # Match rsync -L, including links inside directories reached through links.
    links = subprocess.check_output(["find", "-L", str(root), "-type", "l", "-print0"])
    repair = None
    for item in links.split(b'\0'):
        if not item:
            continue
        link = Path(os.fsdecode(item))
        target = os.readlink(link)
        # Allow only the root-level SDL2 alias for the known SDL2 ABI name.
        if link == root / "libSDL2.so" and target in {
            "libSDL2-2.0.so.0", "./libSDL2-2.0.so.0",
            "/usr/lib/i386-linux-gnu/libSDL2-2.0.so.0",
        }:
            repair = f"{link} -> {target}"
        else:
            sys.exit(f"Unresolved symlink: {link} -> {target}. Restore its target in the Steam installation before staging.")
    if repair:
        print(repair)
except (OSError, subprocess.CalledProcessError) as error:
    sys.exit(f"Cannot inspect Steam installation links under {root}: {error}")
PY_LINKS
)" || return 1
  if [[ -n $repair ]]; then
    [[ -f $sdl ]] || die "$repair. Install libsdl2-2.0-0:i386 or restore the original link target."
    validate_elf32 "$sdl" || die "$repair. Invalid i686 replacement: $sdl"
    command -v readelf >/dev/null || die "$repair. Install binutils to verify the replacement SDL2 ABI."
    dynamic="$(LC_ALL=C readelf -d -- "$sdl")" &&
      grep -Eq '\(SONAME\).*\[libSDL2-2\.0\.so\.0\][[:space:]]*$' <<< "$dynamic" ||
      die "$repair. Replacement lacks the SDL2 ABI SONAME: $sdl"
    excludes=(--exclude='/libSDL2.so')
    printf 'Staging SDL2 replacement for %s using %s\n' "$repair" "$sdl" >&2
  fi
  rsync -aL "${excludes[@]}" -- "$source/" "$destination/" || return 1
  if [[ -n $repair ]]; then
    install -m 755 -- "$sdl" "$destination/libSDL2.so"
  fi
}
stage_installation "$BASE" "$INSTALLS/linux-hlds" /usr/lib/i386-linux-gnu/libSDL2-2.0.so.0
stage_installation "$BASE" "$INSTALLS/linux-rehlds" /usr/lib/i386-linux-gnu/libSDL2-2.0.so.0
extract_elf32 "$DOWNLOADS/rehlds.zip" 'engine_i486\.so' "$INSTALLS/linux-rehlds/engine_i486.so"
for variant in linux-hlds linux-rehlds; do
  mod="$INSTALLS/$variant/ricochet"
  extract_elf32 "$DOWNLOADS/metamod-classic.zip" 'metamod(?:_i386)?\.so' "$mod/addons/classic/dlls/metamod_i386.so"
  extract_elf32 "$DOWNLOADS/metamod-r.zip" 'metamod(?:_i386)?\.so' "$mod/addons/metamod-r/dlls/metamod_i386.so"
done

if [[ -z $UPDATED_DLL && -t 0 ]]; then
  echo
  read -r -p 'Optional Linux i686 ricochet-updated .so path (Enter to skip): ' UPDATED_DLL
fi
UPDATED_PRESENT=0
if [[ -n $UPDATED_DLL ]]; then
  [[ -f $UPDATED_DLL ]] || die "Not a file: $UPDATED_DLL"
  validate_elf32 "$UPDATED_DLL" || die "Invalid ricochet-updated GameDLL."
  for variant in linux-hlds linux-rehlds; do
    cp -- "$UPDATED_DLL" "$INSTALLS/$variant/ricochet/dlls/ricochet_updated.so"
    chmod 755 "$INSTALLS/$variant/ricochet/dlls/ricochet_updated.so"
  done
  UPDATED_PRESENT=1
else
  say "No ricochet-updated GameDLL supplied; only stock Ricochet will be tested."
fi

if [[ -z $REHLDS_CONSOLE_VERSION ]]; then
  say "Probing the staged ReHLDS engine for its console version token."
  PROBE_LOG="$RUNROOT/rehlds-version-probe.log"
  ( cd "$INSTALLS/linux-rehlds" && LD_LIBRARY_PATH="$PWD" \
    timeout --signal=TERM --kill-after=3s 15s \
    ./hlds_linux -console -insecure -nomaster -game ricochet -ip 127.0.0.1 -port 27098 \
    +ip 127.0.0.1 +sv_lan 1 +map "$MAP_1" +version +quit ) \
    > "$PROBE_LOG" 2>&1 || true
  REHLDS_CONSOLE_VERSION="$(python3 - "$REPO/verify/audit" "$PROBE_LOG" <<'PY_REHLDS'
import pathlib, sys
sys.path.insert(0, sys.argv[1])
from evidence import ENGINE

lines = pathlib.Path(sys.argv[2]).read_text(errors='replace').splitlines()
versions = {version for line in lines for name, version, _ in ENGINE.findall(line)
            if name.lower().split() == ['rehlds', 'version']}
if len(versions) == 1:
    print(versions.pop())
PY_REHLDS
)"
  if [[ -z $REHLDS_CONSOLE_VERSION ]]; then
    say "No unique ReHLDS banner version found. ReHLDS engine identity will be unverified. See $PROBE_LOG or supply --rehlds-console-version."
  fi
fi

if [[ -z $VALVE_VERSION ]]; then
  say "Attempting to obtain Valve's original HLDS console version token."
  PROBE_LOG="$RUNROOT/valve-version-probe.log"
  ( cd "$INSTALLS/linux-hlds" && LD_LIBRARY_PATH="$PWD" \
    timeout --signal=TERM --kill-after=3s 15s \
    ./hlds_linux -console -insecure -nomaster -game ricochet -port 27098 \
    +ip 127.0.0.1 +sv_lan 1 +map "$MAP_1" +version +quit ) \
    > "$PROBE_LOG" 2>&1 || true
  VALVE_VERSION="$(python3 - "$PROBE_LOG" <<'PY'
import pathlib,re,sys
s=pathlib.Path(sys.argv[1]).read_text(errors='replace')
a=re.findall(r'\bExe version\s*[:=]?\s*([0-9][0-9A-Za-z.+_-]*)',s,re.I)
print(a[-1] if a else '')
PY
)"
fi
if [[ -z $VALVE_VERSION ]]; then
  say "Could not determine Valve HLDS version automatically. Probe: $RUNROOT/valve-version-probe.log"
  if [[ -t 0 ]]; then
    read -r -p "Enter the HLDS 'Exe version' token (e.g., 1.1.2.7), or Enter to leave unverified: " VALVE_VERSION
  fi
  if [[ -z $VALVE_VERSION ]]; then
    # Honest state: runner will not certify Valve HLDS identity until a
    # version is supplied; it can still collect evidence.
    VALVE_VERSION="unverified-steam-legacy"
    say "Valve HLDS rows will remain INCONCLUSIVE until their version is verified."
  fi
fi

say "Generating a matrix from real installed paths, versions, and available assets."
export RICO_RUNROOT="$RUNROOT" RICO_PLUGIN="$PLUGIN" RICO_PLUGIN_SHA="$PLUGIN_SHA" \
  RICO_PLUGIN_PROVENANCE="$PLUGIN_PROVENANCE" \
  RICO_AUDIT_COMMIT="$AUDIT_COMMIT" RICO_STOCK_REL="$STOCK_REL" \
  RICO_MAP_1="$MAP_1" RICO_MAP_2="$MAP_2" \
  RICO_VALVE_VERSION="$VALVE_VERSION" RICO_REHLDS_VERSION="$REHLDS_VER" \
  RICO_REHLDS_CONSOLE="$REHLDS_CONSOLE_VERSION" RICO_CLASSIC_VERSION="$CLASSIC_VER" \
  RICO_CLASSIC_CONSOLE="$CLASSIC_CONSOLE_VERSION" RICO_MM_R_VERSION="$METAMODR_VER" \
  RICO_MM_R_CONSOLE="$METAMODR_CONSOLE_VERSION" RICO_UPDATED="$UPDATED_PRESENT" \
  RICO_EXPERIMENTAL="$EXPERIMENTAL" RICO_CYCLES="$MAP_CYCLES"
python3 - <<'PY'
import hashlib,json,os,pathlib
E=os.environ
root=pathlib.Path(E['RICO_RUNROOT'])
stock=E['RICO_STOCK_REL']
manifest_path=E['RICO_PLUGIN_PROVENANCE']
provenance=json.loads(pathlib.Path(manifest_path).read_text(encoding='utf-8')) if manifest_path else None
rows=[]
updated=E['RICO_UPDATED']=='1'
for engine,manager in [('hlds','classic'),('rehlds','classic'),('rehlds','metamod-r')] + ([('hlds','metamod-r')] if E['RICO_EXPERIMENTAL']=='1' else []):
    for variant in (['stock','updated'] if updated else ['stock']):
        engine_ver=E['RICO_VALVE_VERSION'] if engine=='hlds' else E['RICO_REHLDS_VERSION']
        manager_ver=E['RICO_CLASSIC_VERSION'] if manager=='classic' else E['RICO_MM_R_VERSION']
        game='ricochet/'+(stock if variant=='stock' else 'dlls/ricochet_updated.so')
        gc=root/'installs'/f'linux-{engine}'/game
        if not gc.is_file():
            raise SystemExit(f'GameDLL absent: {gc}')
        ec={'implementation':'Valve HLDS' if engine=='hlds' else 'ReHLDS',
            'version':engine_ver,'binary':'hlds_linux','modules':['engine_i486.so']}
        # An empty token records a failed probe and prevents release-tag fallback.
        if engine=='rehlds': ec['console_version']=E['RICO_REHLDS_CONSOLE']
        mc={'implementation':'alliedmodders/metamod-hl1' if manager=='classic' else 'Metamod-R',
            'version':manager_ver,'binary':f'ricochet/addons/{manager}/dlls/metamod_i386.so',
            'console_version':E['RICO_CLASSIC_CONSOLE'] if manager=='classic' else E['RICO_MM_R_CONSOLE']}
        rows.append({'id':f'linux-{engine}-{manager}-{variant}','os':'linux',
            'installation':f'installs/linux-{engine}','mod':'ricochet',
            'engine':ec,'metamod':mc,
            'gamedll':{'implementation':'Stock Ricochet' if variant=='stock' else 'ricochet-updated',
                'version':'Steam app 90 steam_legacy' if variant=='stock' else 'operator-supplied',
                'binary':game},
            'plugin':E['RICO_PLUGIN'],'plugin_sha256':E['RICO_PLUGIN_SHA'],
            'maps':[E['RICO_MAP_1'],E['RICO_MAP_2']],
            'map_cycles':int(E['RICO_CYCLES']), 'timeout_seconds':60,
            'port':27045,'experimental':engine=='hlds' and manager=='metamod-r'})
        if provenance is not None:
            rows[-1]['plugin_provenance']=provenance
(root/'matrix.json').write_text(json.dumps({'schema_version':1,'runtimes':rows},indent=2)+'\n')
(root/'sources.json').write_text(json.dumps({
    'repo_commit':E['RICO_AUDIT_COMMIT'],
    'plugin_sha256':E['RICO_PLUGIN_SHA'],
    'download_archives':{p.name:hashlib.sha256(p.read_bytes()).hexdigest()
                          for p in (root.parent.parent/'downloads').glob('*.zip')},
    'stock_relative_gamedll':stock},indent=2)+'\n')
print(f'Generated {len(rows)} runtime configurations:')
for row in rows: print(' -',row['id'])
PY

# The persistent ReHLDS copy uses Metamod-R, while the runner independently
# rewrites liblist.gam in each disposable test stage for every selected manager.
python3 - "$INSTALLS/linux-rehlds/ricochet" "$STOCK_REL" <<'PY'
import pathlib,re,sys
mod=pathlib.Path(sys.argv[1]); stock=sys.argv[2]
p=mod/'liblist.gam'
data=p.read_bytes()
updated,n=re.subn(rb'(?im)^[ \t]*gamedll_linux\s+[^\r\n]*',
                   b'gamedll_linux "addons/metamod-r/dlls/metamod_i386.so"',data)
if n!=1: raise SystemExit('Expected exactly one gamedll_linux entry')
p.write_bytes(updated)
config=mod/'addons/metamod'
config.mkdir(parents=True,exist_ok=True)
(config/'config.ini').write_text(f'gamedll {stock}\nplugins_file addons/metamod/plugins.ini\n')
(config/'plugins.ini').write_text('// No production plugins; audit is used in isolated test stages.\n')
PY
cat > "$RUNROOT/start-rehlds.sh" <<EOF
#!/usr/bin/env bash
set -euo pipefail
cd '$INSTALLS/linux-rehlds'
export LD_LIBRARY_PATH="\$PWD"
exec ./hlds_run -console -insecure -nomaster -game ricochet -port 27015 +ip 127.0.0.1 +sv_lan 1 +maxplayers 8 +map '$MAP_1'
EOF
cat > "$RUNROOT/start-hlds.sh" <<EOF
#!/usr/bin/env bash
set -euo pipefail
cd '$INSTALLS/linux-hlds'
export LD_LIBRARY_PATH="\$PWD"
exec ./hlds_run -console -insecure -nomaster -game ricochet -port 27016 +ip 127.0.0.1 +sv_lan 1 +maxplayers 8 +map '$MAP_1'
EOF
chmod +x "$RUNROOT"/start-{hlds,rehlds}.sh

cat > "$RUNROOT/README.txt" <<EOF
Ricochet runtime audit lab
==========================
Audit runner checkout commit: $AUDIT_COMMIT
Prebuilt audit plugin: $PLUGIN (SHA-256: $PLUGIN_SHA)
Valve base: $BASE (steam_legacy, mod ricochet)
Test configurations: $RUNROOT/matrix.json
ReHLDS standalone server: $RUNROOT/start-rehlds.sh (LAN/loopback only)
Valve HLDS standalone server: $RUNROOT/start-hlds.sh (LAN/loopback only)
Native test log: $RUNROOT/native-fixture/loader.log (if tests enabled)
Native structured report: $RUNROOT/native-fixture/report/report.md (if tests enabled)
Audit report: $RUNROOT/report/report.md
Combined summary: $RUNROOT/audit-summary.md
Raw transcripts: $RUNROOT/report/*/stdout.log and stderr.log

A successful map transition does not imply that stale entity handles were
rejected. The combined summary separates transition callbacks from stale-handle
checks for each map visit. Missing checks are UNOBSERVED, not PASS.
The matrix's 'running' status is not proof that every SDK/host contract holds.
SteamCMD game assets are not included in this installer; they are downloaded
under your Steam terms. No servers are run as root or exposed publicly.
For a manually-operated console use --prepare-only and examine launch.json.
Rerun without provisioning: ./ricochet_audit_ubuntu.sh --rerun latest
Rerun unsuccessful rows: ./ricochet_audit_ubuntu.sh --rerun latest --failed-only
Rerun one row: ./ricochet_audit_ubuntu.sh --rerun $RUN_ID --only linux-rehlds-metamod-r-stock
Rerun results have their own timestamps and preserve the original snapshots.
For client lifecycle probes, add --client-wait 120 and connect/disconnect from
127.0.0.1:27045 when prompted (a remote graphical client cannot reach loopback).
EOF

run_matrix_and_summary
