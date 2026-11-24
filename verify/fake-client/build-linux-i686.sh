#!/usr/bin/env bash
set -euo pipefail
repo=$(cd "$(dirname "$0")/../.." && pwd)
hlsdk="$repo/upstream/hlsdk"
source_file=verify/fake-client/gamedll.cpp
case "${2:-fixture}" in
  fixture) ;;
  stock) source_file=verify/fake-client/ricochet_probe.cpp ;;
  message) source_file=verify/user-message/reentry.cpp ;;
  recorder) source_file=verify/user-message/recorder.cpp ;;
  *) echo 'Expected fixture, stock, message, or recorder' >&2; exit 2 ;;
esac
"${CXX:-c++}" -m32 -std=c++11 -Wall -Wextra -Werror -DLINUX -DGNUC -shared -fPIC \
  -I"$hlsdk/dlls" -I"$hlsdk/engine" -I"$hlsdk/common" -I"$hlsdk/pm_shared" -I"$hlsdk/public" \
  -I"$repo/upstream/metamod-hl1/metamod" "$repo/$source_file" -o "${1:?provide an output .so path}"
