#!/bin/sh
# Zip the extension for the Chrome Web Store and addons.mozilla.org.
# Nothing is uploaded; the archives land in browser-extension/dist/.
#
#   ./browser-extension/package.sh
#
# The Chrome archive drops the development "key" from manifest.json (the
# Web Store assigns its own ID and rejects uploads that carry a key).
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
DIST="$HERE/dist"
VERSION=$(sed -n 's/^ *"version": *"\([^"]*\)".*/\1/p' "$HERE/manifest.json" | head -n 1)
FILES="manifest.json background.js lib.js popup.html popup.js options.html options.js style.css icons/icon-16.png icons/icon-32.png icons/icon-48.png icons/icon-128.png"

command -v zip >/dev/null 2>&1 || { echo "zip is required" >&2; exit 1; }

rm -rf "$DIST"
mkdir -p "$DIST"

build() {
    target=$1
    stage="$DIST/stage-$target"
    mkdir -p "$stage/icons"
    for f in $FILES; do
        cp "$HERE/$f" "$stage/$f"
    done
    if [ "$target" = chrome ]; then
        grep -v '^ *"key":' "$HERE/manifest.json" > "$stage/manifest.json"
    fi
    out="$DIST/kget-extension-$target-$VERSION.zip"
    (cd "$stage" && zip -q -X "$out" $FILES)
    rm -rf "$stage"
    echo "$out"
}

build chrome
build firefox
