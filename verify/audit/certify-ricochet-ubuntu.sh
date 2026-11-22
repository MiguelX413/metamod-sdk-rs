#!/usr/bin/env bash
# Provision a fresh Ubuntu 26.04 LXC runtime audit from an existing Linux i686
# metamod-audit .so. Does not compile the Rust plugin inside the container.
set -Eeuo pipefail
umask 077

REPOSITORY='https://github.com/MiguelX413/metamod-sdk-rs.git'
AUDIT_USER='ricotest'
PLUGIN_INPUT=''
MANIFEST_INPUT=''
BUILD_COMMIT="${AUDIT_BUILD_COMMIT:-}"
REPO_REF="${AUDIT_REPO_REF:-}"
MANIFEST_EXPLICIT=0
SELECTED_RUNTIME=''
UPDATED_DLL_INPUT=''
SKIP_STEAM=0
REQUIRE_SIX=0

usage() {
  cat <<'HELP'
Usage: bash certify-ricochet-ubuntu.sh [options]

Runs on Ubuntu 26.04 x86_64 (including LXC). Start as root on a fresh
container or as an existing non-root user with sudo access.

Requires a PREBUILT Linux i686 libmetamod_audit.so in your home directory.
Does not install a compiler, Rust, or Cargo for the ordinary core audit.

Options:
  --plugin PATH          Path to prebuilt .so (default ~/libmetamod_audit.so)
  --manifest PATH        Existing, truthful audit-build.json (default
                         ~/audit-build.json, if present)
  --build-commit SHA     Attest this exact .so was freshly built from SHA;
                         creates/replaces ~/audit-build.json after clean-build
                         confirmation (backs up the preceding manifest)
  --repo-ref REF         Runner branch, tag, or commit (prompted interactively;
                         default: master; noninteractive default: master)
  --user NAME            Non-root account to create/use if invoked as root
                         (default: ricotest)
  --only RUNTIME_ID      Certify only one runtime; otherwise run full matrix
  --updated-gamedll PATH Linux i686 ricochet-updated GameDLL (six-row matrix);
                         autodetected from ~/ricochet_updated.so when present
  --require-six          Fail early unless an updated GameDLL was supplied
  --skip-steam           Reuse existing downloaded SteamCMD/HLDS assets;
                         useful after a staging failure
  -h, --help             Show this help

An existing default manifest is checked interactively for a newer build,
even if the binary SHA-256 is unchanged. --manifest explicitly selects an
existing manifest; it cannot be combined with --build-commit. In CI, supply
either --manifest PATH or --build-commit SHA with AUDIT_CONFIRM_CLEAN_BUILD=1
ONLY if the source tree was clean at build time. AUDIT_REPO_REF also selects
the runner revision without a prompt.

This script installs sudo/git/python3 as needed to obtain the audit wrapper;
the upstream wrapper installs Ubuntu i386 libraries, SteamCMD, Ricochet,
ReHLDS, and Metamod. Outputs stay under ~/ricochet-audit-lab/runs/.
The local audit-wrapper checkout is updated to the selected origin ref on
EVERY fresh run (only if clean), and its resolved commit is pinned in evidence.
With no updated GameDLL, only the 3 stock-Ricochet configurations are tested.
HELP
}

fail() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }
info() { printf '\n==> %s\n' "$*" >&2; }

while (($#)); do
  case "$1" in
    --plugin|--manifest|--build-commit|--repo-ref|--user|--only|--updated-gamedll)
      (($# >= 2)) || fail "Missing value after $1"
      case "$1" in
        --plugin) PLUGIN_INPUT=$2 ;;
        --manifest) MANIFEST_INPUT=$2; MANIFEST_EXPLICIT=1 ;;
        --build-commit) BUILD_COMMIT=$2 ;;
        --repo-ref) REPO_REF=$2 ;;
        --user) AUDIT_USER=$2 ;;
        --only) SELECTED_RUNTIME=$2 ;;
        --updated-gamedll) UPDATED_DLL_INPUT=$2 ;;
      esac
      shift 2 ;;
    --skip-steam) SKIP_STEAM=1; shift ;;
    --require-six) REQUIRE_SIX=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) fail "Unknown argument: $1 (use --help)" ;;
  esac
done

[[ $(uname -s) == Linux && $(uname -m) == x86_64 ]] || fail 'Requires x86_64 Linux.'
[[ -r /etc/os-release ]] || fail 'Missing /etc/os-release.'
# shellcheck source=/dev/null
source /etc/os-release
[[ ${ID:-} == ubuntu && ${VERSION_ID:-} == 26.04 ]] || \
  fail "Expected Ubuntu 26.04; found ${PRETTY_NAME:-unknown OS}."
[[ $AUDIT_USER =~ ^[a-z_][a-z0-9_-]*$ && $AUDIT_USER != root ]] || \
  fail 'Invalid --user name.'
(( ! MANIFEST_EXPLICIT )) || [[ -z $BUILD_COMMIT ]] || \
  fail '--manifest and --build-commit cannot be combined; choose existing evidence or attest a new build.'
[[ -z $BUILD_COMMIT || $BUILD_COMMIT =~ ^[0-9a-f]{40}$ ]] || \
  fail '--build-commit must be exactly 40 lowercase hex characters.'
[[ -z $REPO_REF || $REPO_REF =~ ^[A-Za-z0-9][A-Za-z0-9._/-]{0,127}$ ]] || \
  fail 'Invalid --repo-ref.'
if [[ -n $SELECTED_RUNTIME ]]; then
  case "$SELECTED_RUNTIME" in
    linux-hlds-classic-stock|linux-rehlds-classic-stock|linux-rehlds-metamod-r-stock|\
    linux-hlds-classic-updated|linux-rehlds-classic-updated|linux-rehlds-metamod-r-updated) ;;
    *) fail "Unsupported --only selection: $SELECTED_RUNTIME" ;;
  esac
fi

# Locate the already-built binary in the CURRENT user's home, including when
# starting as root and moving the binary into the dedicated unprivileged home.
if [[ -z $PLUGIN_INPUT ]]; then
  if [[ -f "$HOME/libmetamod_audit.so" ]]; then
    PLUGIN_INPUT="$HOME/libmetamod_audit.so"
  else
    shopt -s nullglob
    candidates=("$HOME"/libmetamod_audit*.so)
    shopt -u nullglob
    ((${#candidates[@]} == 1)) || fail \
      "Expected one ~/libmetamod_audit*.so, found ${#candidates[@]}; pass --plugin PATH."
    PLUGIN_INPUT=${candidates[0]}
  fi
fi
[[ -f $PLUGIN_INPUT && -r $PLUGIN_INPUT ]] || fail "Cannot read plugin: $PLUGIN_INPUT"
PLUGIN_INPUT="$(realpath -- "$PLUGIN_INPUT")"

# An updated Ricochet GameDLL is a SECOND binary distinct from metamod-audit.
# Never fabricate or automatically download a third-party GameDLL.
if [[ -z $UPDATED_DLL_INPUT && -f $HOME/ricochet_updated.so ]]; then
  UPDATED_DLL_INPUT="$HOME/ricochet_updated.so"
fi
if [[ -n $UPDATED_DLL_INPUT ]]; then
  [[ -f $UPDATED_DLL_INPUT && -r $UPDATED_DLL_INPUT ]] || fail "Cannot read updated GameDLL: $UPDATED_DLL_INPUT"
  UPDATED_DLL_INPUT="$(realpath -- "$UPDATED_DLL_INPUT")"
else
  (( ! REQUIRE_SIX )) || fail '--require-six needs --updated-gamedll PATH (Linux i686 ricochet-updated .so).'
  [[ $SELECTED_RUNTIME != *-updated ]] || fail "The --only selection $SELECTED_RUNTIME needs --updated-gamedll PATH."
fi

if [[ -z $MANIFEST_INPUT && -f "$HOME/audit-build.json" ]]; then
  MANIFEST_INPUT="$HOME/audit-build.json"
fi
if [[ -n $MANIFEST_INPUT ]]; then
  [[ -f $MANIFEST_INPUT && -r $MANIFEST_INPUT ]] || fail "Cannot read manifest: $MANIFEST_INPUT"
  MANIFEST_INPUT="$(realpath -- "$MANIFEST_INPUT")"
fi

if ((EUID == 0)); then
  info "Configuring non-root audit account '$AUDIT_USER'"
  export DEBIAN_FRONTEND=noninteractive
  apt-get update
  apt-get install -y --no-install-recommends sudo git python3 ca-certificates
  if ! id "$AUDIT_USER" >/dev/null 2>&1; then
    useradd --create-home --user-group --shell /bin/bash "$AUDIT_USER"
    usermod -aG sudo "$AUDIT_USER"
    if [[ ! -t 0 ]]; then
      fail "Account created, but cannot set password without a TTY; run 'passwd $AUDIT_USER' and retry."
    fi
    info "Set a password for '$AUDIT_USER' (required for sudo in the audit script)"
    passwd "$AUDIT_USER"
  else
    usermod -aG sudo "$AUDIT_USER"
  fi
  account_home=$(getent passwd "$AUDIT_USER" | cut -d: -f6)
  [[ -d $account_home ]] || fail "Missing home directory for $AUDIT_USER"
  dest_plugin="$account_home/libmetamod_audit.so"
  install -o "$AUDIT_USER" -g "$(id -gn "$AUDIT_USER")" -m 0644 \
    "$PLUGIN_INPUT" "$dest_plugin"
  dest_updated=''
  if [[ -n $UPDATED_DLL_INPUT ]]; then
    dest_updated="$account_home/ricochet_updated.so"
    install -o "$AUDIT_USER" -g "$(id -gn "$AUDIT_USER")" -m 0644 \
      "$UPDATED_DLL_INPUT" "$dest_updated"
  fi
  dest_manifest=''
  if [[ -n $MANIFEST_INPUT ]]; then
    dest_manifest="$account_home/audit-build.json"
    # The account may exist from an earlier run. Do not lose its previous
    # build attestation when transferring a manifest from the invoking user.
    if [[ -f $dest_manifest ]] && ! cmp -s -- "$MANIFEST_INPUT" "$dest_manifest"; then
      previous="$dest_manifest.previous-transfer-$(date -u +%Y%m%dT%H%M%SZ)-$$"
      cp -p -- "$dest_manifest" "$previous"
      info "Preserved existing account provenance: $previous"
    fi
    install -o "$AUDIT_USER" -g "$(id -gn "$AUDIT_USER")" -m 0600 \
      "$MANIFEST_INPUT" "$dest_manifest"
  fi
  # Copy this script because files under /root may be inaccessible to the user.
  [[ -f ${BASH_SOURCE[0]} ]] || fail 'Run a saved script file, not a pipe, when starting as root.'
  script_copy="$account_home/certify-ricochet-ubuntu.sh"
  install -o "$AUDIT_USER" -g "$(id -gn "$AUDIT_USER")" -m 0755 \
    "${BASH_SOURCE[0]}" "$script_copy"
  args=(--plugin "$dest_plugin" --user "$AUDIT_USER")
  if (( MANIFEST_EXPLICIT )) && [[ -n $dest_manifest ]]; then
    args+=(--manifest "$dest_manifest")
  fi
  [[ -z $BUILD_COMMIT ]] || args+=(--build-commit "$BUILD_COMMIT")
  [[ -z $REPO_REF ]] || args+=(--repo-ref "$REPO_REF")
  [[ -z $SELECTED_RUNTIME ]] || args+=(--only "$SELECTED_RUNTIME")
  [[ -z $dest_updated ]] || args+=(--updated-gamedll "$dest_updated")
  (( ! SKIP_STEAM )) || args+=(--skip-steam)
  (( ! REQUIRE_SIX )) || args+=(--require-six)
  info "Continuing as $AUDIT_USER (server processes will NOT run as root)"
  exec runuser -u "$AUDIT_USER" -- env \
    AUDIT_CONFIRM_CLEAN_BUILD="${AUDIT_CONFIRM_CLEAN_BUILD:-}" \
    bash "$script_copy" "${args[@]}"
fi

command -v sudo >/dev/null 2>&1 || fail 'sudo is missing; install it as root first.'
info 'Checking sudo access'
sudo -v || fail 'This account needs sudo permission for Ubuntu package setup.'

if ! command -v git >/dev/null 2>&1 || ! command -v python3 >/dev/null 2>&1; then
  info 'Installing only Git/Python prerequisites needed to obtain the audit script'
  sudo apt-get update
  sudo apt-get install -y --no-install-recommends git python3 ca-certificates
fi

# A build can be recompiled from a new commit without changing its .so bytes.
# Therefore a matching SHA-256 cannot establish that an old manifest is the
# correct source revision for a new build. Prompt before relying on an
# automatically detected manifest, but keep explicitly selected manifests
# authoritative. Never infer a build commit from the current runner checkout.
if (( ! MANIFEST_EXPLICIT )) && [[ -n $MANIFEST_INPUT && -z $BUILD_COMMIT && -t 0 ]]; then
  printf '\nExisting build manifest: %s\n' "$MANIFEST_INPUT" >&2
  python3 - "$MANIFEST_INPUT" <<'PY_SHOW_MANIFEST' || true
import json, pathlib, sys
try:
    m = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding='utf-8'))
    print('Recorded SDK commit:', m.get('sdk_commit', '<missing>'))
    print('Recorded plugin commit:', m.get('audit_plugin_commit', '<missing>'))
    print('Recorded SHA-256:', m.get('sha256', '<missing>'))
except (OSError, ValueError) as exc:
    print(f'Cannot read existing manifest: {exc}')
PY_SHOW_MANIFEST
  printf 'Was this .so BUILT AGAIN after that manifest was created? [y/N] ' >&2
  IFS= read -r rebuilt_answer
  case "$rebuilt_answer" in
    y|Y|yes|YES|Yes)
      info 'The old manifest will be backed up and replaced with truthful new-build provenance.'
      MANIFEST_INPUT=''
      ;;
    ''|n|N|no|NO|No) ;;
    *) fail "Unrecognized answer: $rebuilt_answer" ;;
  esac
fi

if [[ -z $MANIFEST_INPUT || -n $BUILD_COMMIT ]]; then
  if [[ -z $BUILD_COMMIT ]]; then
    [[ -t 0 ]] || fail 'Missing provenance. Pass --build-commit ACTUAL_40_CHARACTER_SHA or --manifest PATH.'
    printf '\nEnter the EXACT 40-character Git commit used to BUILD this .so\n' >&2
    printf '(obtain it on the build machine with git rev-parse HEAD): ' >&2
    IFS= read -r BUILD_COMMIT
  fi
  [[ $BUILD_COMMIT =~ ^[0-9a-f]{40}$ ]] || fail 'Build commit must be 40 lowercase hex characters.'
  if [[ ${AUDIT_CONFIRM_CLEAN_BUILD:-} != 1 ]]; then
    [[ -t 0 ]] || fail 'For noninteractive creation set AUDIT_CONFIRM_CLEAN_BUILD=1 only if the build checkout was clean.'
    printf 'Was the source tree CLEAN when this exact .so was built? [y/N] ' >&2
    IFS= read -r clean_answer
    case "$clean_answer" in
      y|Y|yes|YES|Yes) ;;
      *) fail 'Cannot attest a clean source build. Rebuild from a clean checkout or supply truthful provenance.' ;;
    esac
  fi
  info 'Recording declared prebuilt-plugin provenance in ~/audit-build.json'
  python3 - "$PLUGIN_INPUT" "$BUILD_COMMIT" "$HOME/audit-build.json" <<'PY_CREATE_MANIFEST'
import hashlib
import json
import os
import pathlib
import shutil
import sys
import tempfile
from datetime import datetime, timezone

plugin, commit, target = pathlib.Path(sys.argv[1]), sys.argv[2], pathlib.Path(sys.argv[3])
manifest = {
    'schema_version': 1,
    'sha256': hashlib.sha256(plugin.read_bytes()).hexdigest(),
    'audit_plugin_commit': commit,
    'sdk_commit': commit,
    'working_tree_dirty': False,
}
new_content = json.dumps(manifest, indent=2) + '\n'
if target.exists() and target.read_text(encoding='utf-8') == new_content:
    print('Existing manifest already matches this attested build; no rewrite necessary.')
else:
    if target.exists():
        stamp = datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')
        backup = target.with_name(f'{target.name}.previous-{stamp}-{os.getpid()}')
        shutil.copy2(target, backup)
        print(f'Preserved previous manifest: {backup}')
    fd, tmp_name = tempfile.mkstemp(prefix='.audit-build.', suffix='.tmp', dir=target.parent)
    try:
        with os.fdopen(fd, 'w', encoding='utf-8') as f:
            f.write(new_content)
        os.chmod(tmp_name, 0o600)
        os.replace(tmp_name, target)
    finally:
        if os.path.exists(tmp_name):
            os.unlink(tmp_name)
print(f'Plugin SHA-256: {manifest["sha256"]}')
print(f'Attested source commit: {commit}')
PY_CREATE_MANIFEST
  MANIFEST_INPUT="$HOME/audit-build.json"
fi

# Verify a supplied manifest matches the exact artifact bytes before any
# costly downloads. The revision fields are declarations, not independently
# recoverable facts from an arbitrary prebuilt binary.
python3 - "$PLUGIN_INPUT" "$MANIFEST_INPUT" <<'PY'
import hashlib, json, pathlib, re, sys
plugin, path = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
m = json.loads(path.read_text(encoding='utf-8'))
sha = hashlib.sha256(plugin.read_bytes()).hexdigest()
if (type(m.get('schema_version')) is not int or m['schema_version'] != 1
        or m.get('sha256') != sha
        or m.get('working_tree_dirty') is not False
        or any(not isinstance(m.get(k), str) or not re.fullmatch(r'[0-9a-f]{40}', m[k])
               for k in ('audit_plugin_commit', 'sdk_commit'))):
    raise SystemExit('Plugin provenance is invalid, does not match the .so, or does not attest a clean build.')
print(f'Validated plugin digest: {sha}')
print(f'Plugin source revision: {m["audit_plugin_commit"]}')
print(f'SDK source revision: {m["sdk_commit"]}')
PY

# The exact runner revision is selected independently of the plugin build.
# An interactive fresh run asks; a CI invocation defaults to master.
if [[ -z $REPO_REF ]]; then
  if [[ -t 0 ]]; then
    printf '\nRunner revision (branch, tag, or full commit) [master]: ' >&2
    IFS= read -r REPO_REF
  fi
  REPO_REF="${REPO_REF:-master}"
fi
[[ $REPO_REF =~ ^[A-Za-z0-9][A-Za-z0-9._/-]{0,127}$ ]] || fail 'Invalid runner revision.'

# A dedicated checkout lets this wrapper run from a separate development tree.
REPO="$HOME/ricochet-audit-bootstrap"
if [[ ! -d "$REPO/.git" ]]; then
  if [[ -e $REPO ]]; then fail "$REPO exists but is not a Git checkout"; fi
  info 'Cloning the current metamod-sdk-rs audit bootstrap'
  git clone --depth 1 "$REPOSITORY" "$REPO"
else
  info 'Updating existing local metamod-sdk-rs checkout'
fi
[[ -z $(git -C "$REPO" status --porcelain) ]] || fail \
  "Runner checkout is dirty: $REPO. Clean it or clone a fresh checkout."
# The wrapper must pick up upstream bootstrap fixes after an earlier failed
# installation. A clean checkout is updated without rewriting user changes.
origin_url=$(git -C "$REPO" remote get-url origin) || fail 'Repository has no origin remote.'
case "$origin_url" in
  https://github.com/MiguelX413/metamod-sdk-rs|https://github.com/MiguelX413/metamod-sdk-rs.git|  git@github.com:MiguelX413/metamod-sdk-rs.git) ;;
  *) fail "Unexpected repository remote: $origin_url. Refusing to fetch an untrusted checkout." ;;
esac
info "Fetching runner ref '$REPO_REF' and pinning its exact commit"
git -C "$REPO" fetch --depth=1 origin "$REPO_REF" || fail "Unable to fetch runner ref: $REPO_REF"
git -C "$REPO" checkout --detach FETCH_HEAD || fail 'Could not switch the clean audit wrapper checkout to the fetched revision.'
[[ -z $(git -C "$REPO" status --porcelain) ]] || fail 'Updated runner checkout has uncommitted changes.'
RUNNER_COMMIT=$(git -C "$REPO" rev-parse HEAD)
SCRIPT="$REPO/verify/audit/ricochet_audit_ubuntu.sh"
VALIDATOR="$REPO/verify/audit/elf.py"
[[ -f $SCRIPT && -f $VALIDATOR ]] || fail \
  "Runner checkout is too old or incomplete: $REPO"
info "Checking Linux i686 .so format with the repo's ELF validator"
python3 "$VALIDATOR" "$PLUGIN_INPUT" || fail 'The .so is not a valid ELF32 i386 shared library.'
if [[ -n $UPDATED_DLL_INPUT ]]; then
  python3 "$VALIDATOR" "$UPDATED_DLL_INPUT" || fail 'The updated GameDLL is not a valid ELF32 i386 shared library.'
fi
if [[ -n $UPDATED_DLL_INPUT ]]; then
  info 'Starting stock + ricochet-updated core certification'
else
  info 'Starting stock-Ricochet core certification (to test all six, supply --updated-gamedll PATH)'
fi
info "Pinned audit runner commit: $RUNNER_COMMIT (selected '$REPO_REF')"
args=(--repo-ref "$RUNNER_COMMIT" --plugin "$PLUGIN_INPUT"
      --plugin-provenance "$MANIFEST_INPUT" --certify core)
[[ -z $SELECTED_RUNTIME ]] || args+=(--only "$SELECTED_RUNTIME")
[[ -z $UPDATED_DLL_INPUT ]] || args+=(--updated-gamedll "$UPDATED_DLL_INPUT")
(( ! SKIP_STEAM )) || args+=(--skip-steam)

exec bash "$SCRIPT" "${args[@]}"
