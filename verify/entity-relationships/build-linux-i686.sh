#!/usr/bin/env bash
set -euo pipefail
[[ $# == 1 ]] || { echo "Usage: $0 NATIVE_FIXTURE_SO" >&2; exit 2; }
output=$(realpath -m -- "$1")
repo=$(cd "$(dirname "$0")/../.." && pwd)
cd "$repo"
cargo build --locked --release --target i686-unknown-linux-gnu \
    -p engine-demo --example entity_relationships
hlsdk="$repo/upstream/hlsdk"
"${CXX:-c++}" -m32 -std=c++11 -Wall -Wextra -Werror -DLINUX -DGNUC -shared -fPIC \
    -I"$hlsdk/dlls" -I"$hlsdk/engine" -I"$hlsdk/common" -I"$hlsdk/pm_shared" \
    -I"$hlsdk/public" -I"$repo/upstream/metamod-hl1/metamod" \
    verify/entity-relationships/fixture.cpp -o "$output"
