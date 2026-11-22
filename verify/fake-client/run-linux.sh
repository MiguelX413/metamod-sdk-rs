#!/usr/bin/env bash
# Run against saved audit snapshots on a disposable test host.
set -euo pipefail
[[ $# == 3 ]] || { echo "Usage: $0 SAVED_INSTALLS FIXTURE_SO NEW_OUTPUT_DIR" >&2; exit 2; }
base=$(realpath -e -- "$1")
fixture=$(realpath -e -- "$2")
output=$(realpath -- "$3")
case "$output/" in
  "$base/"*) echo "Output must be outside the saved installations." >&2; exit 2 ;;
esac
mkdir -- "$output"
ulimit -c 0
failed=0
for engine in linux-hlds linux-rehlds; do
  source_dir="$base/$engine"
  unsafe_entry=$(find "$source_dir" ! -type f ! -type d -print -quit)
  if [[ -n $unsafe_entry ]]; then
    echo "Source contains a link or special file: $unsafe_entry" >&2
    exit 1
  fi
  cp -a --reflink=auto -- "$source_dir" "$output/$engine"
  (
    cd "$output/$engine"
    install -m 755 -- "$fixture" ricochet/dlls/fake_fixture.so
    mkdir -p ricochet/addons/mm_fixture
    : >ricochet/addons/mm_fixture/plugins.ini
    printf 'gamedll dlls/fake_fixture.so\nplugins_file addons/mm_fixture/plugins.ini\n' >ricochet/addons/mm_fixture/config.ini
    cp -- ricochet/addons/classic/dlls/metamod_i386.so ricochet/addons/classic/dlls/metamod.so
    sed -i '/^gamedll_linux /c\gamedll_linux "addons/classic/dlls/metamod.so"' ricochet/liblist.gam
    mkfifo "$output/$engine.in"
    exec 3<>"$output/$engine.in"
    LD_LIBRARY_PATH="$PWD" timeout --kill-after=3s 20s ./hlds_linux \
      -console -insecure -nomaster -game ricochet -ip 127.0.0.1 -port 27057 \
      +ip 127.0.0.1 +sv_lan 1 +maxplayers 2 +deathmatch 1 \
      +localinfo mm_configfile addons/mm_fixture/config.ini \
      +localinfo mm_pluginsfile addons/mm_fixture/plugins.ini \
      +localinfo mm_gamedll dlls/fake_fixture.so +map rc_arena <&3 >"$output/$engine.log" 2>&1 &
    pid=$!
    for ((n=0; n<30; ++n)); do
      if grep -Eq '^FAKE_FIXTURE (PASS|FAIL)|Host_Error' "$output/$engine.log" || ! kill -0 "$pid" 2>/dev/null; then
        break
      fi
      sleep .4
    done
    printf 'version\nmeta version\nquit\n' >&3
    wait "$pid" || exit $?
    grep -q '^FAKE_FIXTURE PASS ' "$output/$engine.log"
  ) &
  wait "$!" || failed=1
  grep -E 'FAKE_FIXTURE|Host_Error|ReHLDS version|Exe build' "$output/$engine.log" || true
done
exit "$failed"
