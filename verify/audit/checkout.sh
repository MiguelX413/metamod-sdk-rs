#!/usr/bin/env bash
# Git checkout validation for Ubuntu provisioning and reruns.

check_checkout() {
  local repo="$1" expected="${2:-}" allow_uninitialized="${3:-0}" head status modules
  head="$(git -C "$repo" rev-parse --verify 'HEAD^{commit}')" || return 1
  [[ -z $expected || $head == "$expected" ]] || {
    echo "Runner checkout mismatch: expected $expected, got $head" >&2
    return 1
  }
  status="$(git -C "$repo" status --porcelain --untracked-files=all --ignore-submodules=none)" || return 1
  [[ -z $status ]] || { echo "Runner checkout is dirty: $repo" >&2; return 1; }
  modules="$(git -C "$repo" submodule status --recursive)" || return 1
  if grep -Eq '^[+U]' <<< "$modules" || {
    (( ! allow_uninitialized )) && grep -q '^-' <<< "$modules"
  }; then
    echo "Runner checkout has mismatched or incomplete pinned submodules: $repo" >&2
    return 1
  fi
}

select_checkout() {
  local repo="$1" ref="$2" selected origin
  check_checkout "$repo" '' 1 || return 1
  origin="$(git -C "$repo" remote get-url origin)" || return 1
  case "$origin" in
    https://github.com/MiguelX413/metamod-sdk-rs|https://github.com/MiguelX413/metamod-sdk-rs.git|git@github.com:MiguelX413/metamod-sdk-rs.git) ;;
    *) echo "Unexpected runner origin: $origin" >&2; return 1 ;;
  esac
  git -C "$repo" fetch --depth=1 -- origin "$ref" || return 1
  selected="$(git -C "$repo" rev-parse --verify 'FETCH_HEAD^{commit}')" || return 1
  if [[ $ref =~ ^[0-9a-fA-F]{40}$ && ${ref,,} != "$selected" ]]; then
    echo "Fetched commit does not match requested commit: $ref" >&2
    return 1
  fi
  git -C "$repo" checkout --detach "$selected" || return 1
  git -C "$repo" submodule update --init --recursive --checkout || return 1
  check_checkout "$repo" "$selected"
}
