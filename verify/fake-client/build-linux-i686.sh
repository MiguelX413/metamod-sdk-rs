#!/usr/bin/env bash
set -euo pipefail
repo=$(cd "$(dirname "$0")/../.." && pwd)
hlsdk="$repo/upstream/hlsdk"
"${CXX:-c++}" -m32 -std=c++11 -Wall -Wextra -Werror -DLINUX -DGNUC -shared -fPIC \
  -I"$hlsdk/dlls" -I"$hlsdk/engine" -I"$hlsdk/common" -I"$hlsdk/pm_shared" -I"$hlsdk/public" \
  "$repo/verify/fake-client/gamedll.cpp" -o "${1:?provide an output .so path}"
