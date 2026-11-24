#!/usr/bin/env bash
set -euo pipefail

repo=$(cd "$(dirname "$0")/.." && pwd)
metamod=$(realpath "${1:-$repo/upstream/metamod-hl1}")
hlsdk=$(realpath "${2:-$repo/upstream/hlsdk}")
cd "$repo"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

if [[ -n ${METAMOD_NATIVE_EVIDENCE_DIR:-} ]]; then
    if [[ "$metamod" != "$repo/upstream/metamod-hl1" || "$hlsdk" != "$repo/upstream/hlsdk" ]]; then
        echo 'Native evidence requires the pinned repository header paths.' >&2
        exit 1
    fi
    python3 verify/audit/native_identity.py --source "$METAMOD_NATIVE_EVIDENCE_DIR/source-before.json"
fi

export CARGO_TARGET_DIR="$work/target"
if [[ ${METAMOD_SANITIZE_PLUGIN:-0} == 1 ]]; then
    # The loader supplies one Clang ASan runtime for the whole process,
    # including the instrumented cdylibs loaded by dlopen.
    clang=${METAMOD_SANITIZER_CLANG:-clang}
    export CXX="${METAMOD_SANITIZER_CXX:-clang++}"
    METAMOD_SANITIZE_LOADER=1
    RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-Zsanitizer=address -Zexternal-clangrt -Clinker=$clang" \
        cargo +nightly build --locked --release --target i686-unknown-linux-gnu -p hello-world -p engine-demo -p metamod-audit
else
    cargo build --locked --release --target i686-unknown-linux-gnu -p hello-world -p engine-demo -p metamod-audit
fi
cargo build --locked --target i686-unknown-linux-gnu -p metamod-sys --example layout

includes=(-I"$metamod/metamod" -I"$hlsdk/dlls" -I"$hlsdk/engine"
          -I"$hlsdk/common" -I"$hlsdk/pm_shared" -I"$hlsdk/public")
"${CXX:-c++}" -m32 -std=c++11 -DLINUX -DGNUC "${includes[@]}" verify/layout.cpp -o "$work/layout"
sanitizers=()
if [[ ${METAMOD_SANITIZE_LOADER:-0} == 1 ]]; then
    sanitizers=(-fsanitize=address,undefined -fno-sanitize-recover=all -fno-omit-frame-pointer
                -DMETAMOD_LOADER_LSAN)
fi
"${CXX:-c++}" -m32 -std=c++11 -DLINUX -DGNUC "${includes[@]}" "${sanitizers[@]}" verify/load.cpp -pthread -ldl -o "$work/load"
if [[ -n ${METAMOD_NATIVE_EVIDENCE_DIR:-} ]]; then
    export METAMOD_SANITIZE_LOADER
    python3 verify/audit/native_identity.py "$METAMOD_NATIVE_EVIDENCE_DIR" "$work"
fi
"$work/layout" > "$work/c-layout"
"$CARGO_TARGET_DIR/i686-unknown-linux-gnu/debug/examples/layout" > "$work/rust-layout"
diff -u "$work/c-layout" "$work/rust-layout"
for example in hello-world engine-demo metamod-audit; do
    plugin="$CARGO_TARGET_DIR/i686-unknown-linux-gnu/release/lib${example//-/_}.so"
    if [[ ${METAMOD_SANITIZE_PLUGIN:-0} == 1 ]]; then
        nm -D --undefined-only "$plugin" > "$work/imports"
        grep -Eq ' U __asan_(load|report_load)' "$work/imports"
    fi
    readelf -h "$plugin" | tee "$work/elf"
    grep -Eq 'Class: +ELF32' "$work/elf"
    grep -Eq 'Machine: +Intel 80386' "$work/elf"
    nm -D --defined-only "$plugin" | awk '$2 == "T" {print $3}' | sort > "$work/exports"
    printf '%s\n' GiveFnptrsToDll Meta_Attach Meta_Detach Meta_Query | diff -u - "$work/exports"
    if [[ ${METAMOD_SANITIZE_LOADER:-0} == 1 ]]; then
        ASAN_OPTIONS="${ASAN_OPTIONS:+$ASAN_OPTIONS:}detect_leaks=1" "$work/load" "$plugin" "$example"
    else
        "$work/load" "$plugin" "$example"
    fi
done
echo 'ABI and loader checks passed.'
