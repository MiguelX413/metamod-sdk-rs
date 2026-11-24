#!/usr/bin/env bash
# CI wrapper for the existing Linux ABI and loader verification.
set -euo pipefail

output=$1
fixture=$2
mkdir -p "$output"
output=$(realpath "$output")
if [[ -e "$output/runtime.json" || -e "$output/build.json" || -e "$output/loader.log" ]]; then
    echo 'Use a fresh output directory for each native run.' >&2
    exit 1
fi
export METAMOD_NATIVE_EVIDENCE_DIR="$output"
METAMOD_NATIVE_RUN_ID=$(python3 -c 'import uuid; print(uuid.uuid4())')
export METAMOD_NATIVE_RUN_ID
status=0
bash verify/linux-i686.sh > "$output/loader.log" 2>&1 || status=$?
cat "$output/loader.log"
python3 - "$output" "$fixture" "$status" <<'PYTHON'
import hashlib, json, os, pathlib, sys
folder = pathlib.Path(sys.argv[1])
runtime = dict(id=sys.argv[2], os='linux', architecture='i686', exit_status=int(sys.argv[3]))
if (folder / 'build.json').exists():
    sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    runtime['native_identity'] = dict(run_id=os.environ['METAMOD_NATIVE_RUN_ID'],
        build_sha256=sha(folder / 'build.json'), logs={'loader.log': sha(folder / 'loader.log')})
(folder / 'runtime.json').write_text(json.dumps(runtime) + '\n')
PYTHON
python3 verify/audit/runner.py import "$output/runtime.json" "$output/report" \
    "$output/loader.log" --native-fixture --require-native-suite
