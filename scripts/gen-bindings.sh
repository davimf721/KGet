#!/bin/bash
#
# Build kget-ffi and generate its Swift and Kotlin bindings into bindings/.
#
# Usage: scripts/gen-bindings.sh [--debug] [swift] [kotlin]
#   (no language = both). Honours CARGO_TARGET_DIR.
#
# Output (gitignored, regenerate after changing crates/kget-ffi):
#   bindings/swift/KgetFFI.swift, KgetFFIFFI.h, KgetFFIFFI.modulemap
#   bindings/kotlin/io/github/davimf721/kget/kget_ffi.kt
#   bindings/lib/libkget_ffi.{a,dylib|so}

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PROFILE=release
LANGS=()
for arg in "$@"; do
    case "$arg" in
        --debug) PROFILE=debug ;;
        swift|kotlin) LANGS+=("$arg") ;;
        -h|--help) sed -n '3,12p' "$0"; exit 0 ;;
        *) echo "Unknown argument: $arg" >&2; exit 2 ;;
    esac
done
[ ${#LANGS[@]} -eq 0 ] && LANGS=(swift kotlin)

CARGO_FLAGS=(-p kget-ffi)
[ "$PROFILE" = release ] && CARGO_FLAGS+=(--release)

TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
case "$TARGET_DIR" in /*) ;; *) TARGET_DIR="$ROOT/$TARGET_DIR" ;; esac
case "$(uname -s)" in
    Darwin) DYLIB=libkget_ffi.dylib ;;
    MINGW*|MSYS*|CYGWIN*) DYLIB=kget_ffi.dll ;;
    *) DYLIB=libkget_ffi.so ;;
esac

echo "==> Building kget-ffi ($PROFILE)"
cargo build "${CARGO_FLAGS[@]}" --lib
cargo build "${CARGO_FLAGS[@]}" --bin uniffi-bindgen

LIB="$TARGET_DIR/$PROFILE/$DYLIB"
BINDGEN="$TARGET_DIR/$PROFILE/uniffi-bindgen"
[ -f "$LIB" ] || { echo "Missing $LIB" >&2; exit 1; }

OUT="$ROOT/bindings"
mkdir -p "$OUT/lib"
cp "$LIB" "$OUT/lib/"
[ -f "$TARGET_DIR/$PROFILE/libkget_ffi.a" ] && cp "$TARGET_DIR/$PROFILE/libkget_ffi.a" "$OUT/lib/"

# crates/kget-ffi/uniffi.toml (module names) is picked up automatically.
for lang in "${LANGS[@]}"; do
    echo "==> Generating $lang bindings"
    rm -rf "${OUT:?}/$lang"
    FORMAT=()
    [ "$lang" = kotlin ] && ! command -v ktlint >/dev/null 2>&1 && FORMAT=(--no-format)
    "$BINDGEN" generate --library "$LIB" --language "$lang" --out-dir "$OUT/$lang" ${FORMAT[@]+"${FORMAT[@]}"}
done

echo "==> Bindings written to $OUT"
