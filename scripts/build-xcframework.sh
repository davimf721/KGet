#!/bin/bash
#
# Build bindings/KgetFFI.xcframework from the kget-ffi staticlib for every
# Apple target whose Rust standard library is installed:
#   macOS   aarch64-apple-darwin + x86_64-apple-darwin (lipo'd)
#   iOS     aarch64-apple-ios
#   iOS sim aarch64-apple-ios-sim + x86_64-apple-ios (lipo'd)
# Missing targets are skipped with a message (`rustup target add <triple>`).
# Also generates the Swift bindings (bindings/swift/KgetFFI.swift), which an
# app compiles next to the XCFramework.
#
# Usage: scripts/build-xcframework.sh   (macOS with Xcode only)

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [ "$(uname -s)" != Darwin ] || ! command -v xcodebuild >/dev/null 2>&1; then
    echo "SKIP: XCFrameworks need macOS with Xcode (xcodebuild)"
    exit 0
fi

TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
case "$TARGET_DIR" in /*) ;; *) TARGET_DIR="$ROOT/$TARGET_DIR" ;; esac
OUT="$ROOT/bindings"
WORK="$OUT/xcframework-build"

installed() {
    # Without rustup, assume only the host target is available.
    if command -v rustup >/dev/null 2>&1; then
        rustup target list --installed 2>/dev/null | grep -qx "$1"
    else
        [ "$1" = "$(rustc -vV | sed -n 's/^host: //p')" ]
    fi
}

# Build the staticlib for each available triple of a platform and lipo them
# into $WORK/<platform>/libkget_ffi.a. Prints the path, or nothing.
build_platform() {
    local platform="$1"; shift
    local libs=()
    for triple in "$@"; do
        if installed "$triple"; then
            echo "==> Building $triple" >&2
            cargo build -p kget-ffi --lib --release --target "$triple" >&2
            libs+=("$TARGET_DIR/$triple/release/libkget_ffi.a")
        else
            echo "SKIP: $triple not installed (rustup target add $triple)" >&2
        fi
    done
    [ ${#libs[@]} -eq 0 ] && return 0
    mkdir -p "$WORK/$platform"
    lipo -create "${libs[@]}" -output "$WORK/$platform/libkget_ffi.a"
    echo "$WORK/$platform/libkget_ffi.a"
}

rm -rf "$WORK" "$OUT/KgetFFI.xcframework"
mkdir -p "$WORK"

# Swift sources + C header/modulemap (generated from the host build).
"$ROOT/scripts/gen-bindings.sh" swift

HEADERS="$WORK/headers"
mkdir -p "$HEADERS"
cp "$OUT/swift/KgetFFIFFI.h" "$HEADERS/"
# Xcode looks for `module.modulemap` inside the XCFramework headers.
cp "$OUT/swift/KgetFFIFFI.modulemap" "$HEADERS/module.modulemap"

ARGS=()
for spec in \
    "macos aarch64-apple-darwin x86_64-apple-darwin" \
    "ios aarch64-apple-ios" \
    "ios-simulator aarch64-apple-ios-sim x86_64-apple-ios"; do
    # shellcheck disable=SC2086
    lib="$(build_platform $spec)"
    [ -n "$lib" ] && ARGS+=(-library "$lib" -headers "$HEADERS")
done

if [ ${#ARGS[@]} -eq 0 ]; then
    echo "No Apple target could be built" >&2
    exit 1
fi

xcodebuild -create-xcframework "${ARGS[@]}" -output "$OUT/KgetFFI.xcframework"
rm -rf "$WORK"
echo "==> $OUT/KgetFFI.xcframework (+ $OUT/swift/KgetFFI.swift)"
