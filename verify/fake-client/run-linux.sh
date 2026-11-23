#!/usr/bin/env bash
# Run against saved audit snapshots on a disposable test host.
set -euo pipefail
[[ $# -ge 3 && $# -le 5 ]] || { echo "Usage: $0 SAVED_INSTALLS FIXTURE_SO NEW_OUTPUT_DIR [MODE [GAMEDLL_SO]]" >&2; exit 2; }
base=$(realpath -e -- "$1")
fixture=$(realpath -e -- "$2")
output=$(realpath -- "$3")
case "$output/" in
  "$base/"*) echo "Output must be outside the saved installations." >&2; exit 2 ;;
esac
mode=${4:-fixture}
case "$mode" in
  fixture) marker=FAKE_FIXTURE ;;
  stock) marker=FAKE_STOCK ;;
  map) marker=FAKE_MAP ;;
  reentry) marker=FAKE_REENTRY ;;
  allocation-reentry)
    marker=FAKE_REENTRY
    gamedll_fixture=$(realpath -e -- "${5:?allocation-reentry requires the fixture GameDLL}")
    ;;
  message-post-end|message-nested-begin|message-nested-write|message-nested-end)
    marker=MESSAGE_PROBE
    gamedll_fixture=$(realpath -e -- "${5:?message mode requires the fixture GameDLL}")
    ;;
  *) echo "Unknown mode: $mode" >&2; exit 2 ;;
esac
if [[ $# == 5 && $mode != allocation-reentry && $mode != message-* ]]; then
  echo 'A separate GameDLL is only used in allocation-reentry and message modes.' >&2
  exit 2
fi
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
    mkdir -p ricochet/addons/mm_fixture
    if [[ $mode != fixture ]]; then
      gamedll=dlls/ricochet.so
      install -m 755 -- "$fixture" ricochet/addons/mm_fixture/ricochet_probe.so
      printf 'linux addons/mm_fixture/ricochet_probe.so\n' >ricochet/addons/mm_fixture/plugins.ini
    else
      gamedll=dlls/fake_fixture.so
      install -m 755 -- "$fixture" "ricochet/$gamedll"
      : >ricochet/addons/mm_fixture/plugins.ini
    fi
    if [[ $mode == allocation-reentry ]]; then
      gamedll=dlls/fake_fixture.so
      install -m 755 -- "$gamedll_fixture" "ricochet/$gamedll"
      export METAMOD_FAKE_ALLOCATION_REENTRY=1
    else
      unset METAMOD_FAKE_ALLOCATION_REENTRY
    fi
    if [[ $mode == message-* ]]; then
      gamedll=dlls/fake_fixture.so
      install -m 755 -- "$gamedll_fixture" "ricochet/$gamedll"
      export METAMOD_MESSAGE_PROBE="${mode#message-}"
    else
      unset METAMOD_MESSAGE_PROBE
    fi
    printf 'gamedll %s\nplugins_file addons/mm_fixture/plugins.ini\n' "$gamedll" >ricochet/addons/mm_fixture/config.ini
    cp -- ricochet/addons/classic/dlls/metamod_i386.so ricochet/addons/classic/dlls/metamod.so
    sed -i '/^gamedll_linux /c\gamedll_linux "addons/classic/dlls/metamod.so"' ricochet/liblist.gam
    mkfifo "$output/$engine.in"
    exec 3<>"$output/$engine.in"
    LD_LIBRARY_PATH="$PWD" timeout --kill-after=3s 20s ./hlds_linux \
      -console -insecure -nomaster -game ricochet -ip 127.0.0.1 -port 27057 \
      +ip 127.0.0.1 +sv_lan 1 +maxplayers 2 +deathmatch 1 \
      +localinfo mm_configfile addons/mm_fixture/config.ini \
      +localinfo mm_pluginsfile addons/mm_fixture/plugins.ini \
      +localinfo mm_gamedll "$gamedll" +map rc_arena <&3 >"$output/$engine.log" 2>&1 &
    pid=$!
    sent_change=0
    for ((n=0; n<30; ++n)); do
      if grep -Eq "^$marker (PASS|FAIL|outer completed)|Host_Error|FATAL ERROR" "$output/$engine.log" || ! kill -0 "$pid" 2>/dev/null; then
        break
      fi
      if [[ $mode == map ]] && (( ! sent_change )) && grep -q '^FAKE_MAP READY$' "$output/$engine.log"; then
        # One command-buffer line places changelevel ahead of the kick the
        # probe queues while handling sdk_fake_map_probe.
        printf 'sdk_fake_map_probe; changelevel rc_deathmatch\n' >&3
        sent_change=1
      fi
      sleep .4
    done
    printf 'version\nmeta version\nstatus\nquit\n' >&3
    status=0
    wait "$pid" || status=$?
    printf 'exit_status=%s\n' "$status" >"$output/$engine.exit"
    if [[ $mode == message-* ]]; then
      log="$output/$engine.log"
      grep -Fx "MESSAGE_HOOK ${mode#message-}" "$log"
      case "$mode" in
        message-post-end)
          ((status == 0))
          grep -Fx 'MESSAGE_HOOK second message completed' "$log"
          grep -Fx 'MESSAGE_PROBE outer completed' "$log"
          ;;
        message-nested-write)
          ((status == 0))
          grep -Fx 'MESSAGE_HOOK nested write returned' "$log"
          grep -Fx 'MESSAGE_PROBE outer completed' "$log"
          ;;
        *)
          ((status != 0 && status != 124 && status != 137))
          ! grep -q '^MESSAGE_PROBE outer completed$' "$log" || exit 1
          case "$mode" in
            message-nested-begin) grep -Ei 'new message started|already started' "$log" ;;
            message-nested-end)
              grep -Fx 'MESSAGE_HOOK nested end returned' "$log"
              grep -Ei 'no active message|without MessageBegin' "$log"
              ;;
          esac
          ;;
      esac
      exit 0
    fi
    ((status == 0)) || exit "$status"
    grep -q "^$marker PASS " "$output/$engine.log"
    if [[ $mode == reentry || $mode == allocation-reentry ]]; then
      grep -E '^map[[:space:]]*:[[:space:]]+rc_deathmatch([[:space:]]|$)' "$output/$engine.log"
      grep -A1 -F 'Dropped SDKReentryProbe from server' "$output/$engine.log" |
        grep -F 'Reason:  Dropping fakeclient on level change'
    fi
    if [[ $mode == allocation-reentry ]]; then
      grep -Fx 'FAKE_REENTRY EVENT userinfo during allocation; private data absent' "$output/$engine.log"
      grep -Fx 'FAKE_REENTRY EVENT second map activated before userinfo returned' "$output/$engine.log"
      grep -Fx 'FAKE_REENTRY EVENT fixture disconnected client without private data' "$output/$engine.log"
      grep -Fx 'FAKE_REENTRY EVENT adapter setup closure was not entered' "$output/$engine.log"
      ! grep -q '^FAKE_REENTRY FAIL ' "$output/$engine.log" || exit 1
    fi
    if [[ $mode == map ]]; then
      grep -E '^map[[:space:]]*:[[:space:]]+rc_deathmatch([[:space:]]|$)' "$output/$engine.log"
      # Both hosts dispatch the map-triggered disconnect before deactivation.
      # Corroborate why the old slot was freed outside the SDK's pre-hook.
      grep -A1 -F 'Dropped SDKMapOld from server' "$output/$engine.log" |
        grep -F 'Reason:  Dropping fakeclient on level change'
      grep -A1 -F 'Dropped SDKMapReplacement from server' "$output/$engine.log" |
        grep -F 'Reason:  Kicked'
    fi
  ) &
  wait "$!" || failed=1
  grep -E "$marker|Host_Error|ReHLDS version|Exe build" "$output/$engine.log" || true
done
exit "$failed"
