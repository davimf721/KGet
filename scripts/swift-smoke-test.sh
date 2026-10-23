#!/bin/bash
#
# Compile and run a tiny Swift program against the generated bindings
# (run scripts/gen-bindings.sh first). Skips when swiftc is unavailable.
#
# Usage: scripts/swift-smoke-test.sh [work-dir]
#   work-dir defaults to a fresh mktemp directory (keep it off exFAT/network
#   volumes: the binary and its config dir are written there).

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINDINGS="$ROOT/bindings"

if ! command -v swiftc >/dev/null 2>&1; then
    echo "SKIP: swiftc not found"
    exit 0
fi
case "$(uname -s)" in
    Darwin) DYLIB=libkget_ffi.dylib ;;
    *) DYLIB=libkget_ffi.so ;;
esac
for f in swift/KgetFFI.swift swift/KgetFFIFFI.h swift/KgetFFIFFI.modulemap "lib/$DYLIB"; do
    [ -f "$BINDINGS/$f" ] || { echo "Missing bindings/$f; run scripts/gen-bindings.sh swift" >&2; exit 1; }
done

TMPDIR="${TMPDIR:-/tmp}"
WORK="${1:-$(mktemp -d "${TMPDIR%/}/kget-ffi-smoke.XXXXXX")}"
mkdir -p "$WORK"
# Link the dynamic library from a directory of its own so the static one
# next to it in bindings/lib is not picked up.
cp "$BINDINGS/lib/$DYLIB" "$WORK/"

echo "==> Compiling Swift smoke test in $WORK"
swiftc -O \
    -module-name KgetSmoke \
    -I "$BINDINGS/swift" \
    -Xcc -fmodule-map-file="$BINDINGS/swift/KgetFFIFFI.modulemap" \
    -L "$WORK" -lkget_ffi \
    -Xlinker -rpath -Xlinker "$WORK" \
    "$BINDINGS/swift/KgetFFI.swift" \
    "$ROOT/crates/kget-ffi/tests/swift/main.swift" \
    -o "$WORK/kget-smoke"

echo "==> Running"
"$WORK/kget-smoke" "$WORK/config"
