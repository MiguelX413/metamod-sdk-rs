#!/usr/bin/env bash
set -euo pipefail
repo=$(cd "$(dirname "$0")/../.." && pwd)
hlsdk="$repo/upstream/hlsdk"
source_file=gamedll.cpp
case "${2:-fixture}" in
  fixture) ;;
  stock) source_file=ricochet_probe.cpp ;;
  *) echo 'Expected fixture or stock' >&2; exit 2 ;;
esac
"${CXX:-c++}" -m32 -std=c++11 -Wall -Wextra -Werror -DLINUX -DGNUC -shared -fPIC \
  -I"$hlsdk/dlls" -I"$hlsdk/engine" -I"$hlsdk/common" -I"$hlsdk/pm_shared" -I"$hlsdk/public" \
  -I"$repo/upstream/metamod-hl1/metamod" "$repo/verify/fake-client/$source_file" -o "${1:?provide an output .so path}"
