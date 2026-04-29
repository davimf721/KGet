#!/bin/bash
#
# KGet Automated Release Script
# ==============================
# Builds all platform binaries, creates a GitHub release, and publishes to crates.io.
#
# Usage:
#   ./release.sh                  # full release (build + GitHub + crates.io)
#   ./release.sh --build-only     # only build binaries
#   ./release.sh --github-only    # only create GitHub release (binaries must exist)
#   ./release.sh --crates-only    # only publish to crates.io
#   ./release.sh --skip-crates    # build + GitHub release, skip crates.io
#   ./release.sh --dry-run        # show what would happen, don't publish
#
# Prerequisites:
#   - cargo, rustup
#   - gh (GitHub CLI) logged in
#   - cross (cargo install cross --git https://github.com/cross-rs/cross) + Docker
#     OR cargo-zigbuild (cargo install cargo-zigbuild) + zig (brew install zig)
#   - swift + xcode (for macOS DMG)
#   - create-dmg (brew install create-dmg, optional)
#

set -euo pipefail

# ============================================================================
# Colors & helpers
# ============================================================================

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
BOLD='\033[1m'
NC='\033[0m'

info()    { echo -e "${BLUE}==>${NC} ${BOLD}$*${NC}"; }
success() { echo -e "${GREEN}✓${NC} $*"; }
warn()    { echo -e "${YELLOW}⚠${NC}  $*"; }
error()   { echo -e "${RED}✗${NC} $*" >&2; }
die()     { error "$*"; exit 1; }
sep()     { echo -e "${BLUE}$(printf '─%.0s' {1..56})${NC}"; }

# ============================================================================
# Argument parsing
# ============================================================================

DO_BUILD=true
DO_GITHUB=true
DO_CRATES=true
DRY_RUN=false

for arg in "$@"; do
    case "$arg" in
        --build-only)   DO_GITHUB=false; DO_CRATES=false ;;
        --github-only)  DO_BUILD=false;  DO_CRATES=false ;;
        --crates-only)  DO_BUILD=false;  DO_GITHUB=false ;;
        --skip-crates)  DO_CRATES=false ;;
        --dry-run)      DRY_RUN=true ;;
        --help|-h)
            sed -n '/^# Usage:/,/^$/p' "$0" | grep -v '^#$' | sed 's/^# \?//'
            exit 0 ;;
        *) die "Unknown argument: $arg. Use --help for usage." ;;
    esac
done

# ============================================================================
# Read version from Cargo.toml
# ============================================================================

VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
RELEASE_DIR="release"
TAG="v${VERSION}"

echo ""
echo -e "${BOLD}KGet Release Script${NC}"
sep
echo -e "  Version  : ${BOLD}${VERSION}${NC}"
echo -e "  Tag      : ${BOLD}${TAG}${NC}"
echo -e "  Build    : $( $DO_BUILD  && echo "${GREEN}yes${NC}" || echo "no")"
echo -e "  GitHub   : $( $DO_GITHUB && echo "${GREEN}yes${NC}" || echo "no")"
echo -e "  crates.io: $( $DO_CRATES && echo "${GREEN}yes${NC}" || echo "no")"
$DRY_RUN && echo -e "  ${YELLOW}DRY RUN — nothing will be published${NC}"
sep
echo ""

# ============================================================================
# Sanity checks
# ============================================================================

info "Checking prerequisites..."

command -v cargo &>/dev/null || die "cargo not found"
command -v rustup &>/dev/null || die "rustup not found"

if $DO_GITHUB; then
    command -v gh &>/dev/null || die "gh CLI not found. Install: brew install gh && gh auth login"
    gh auth status &>/dev/null    || die "gh CLI not authenticated. Run: gh auth login"
fi

if $DO_BUILD; then
    # Detect cross-compilation tool
    USE_CROSS=false
    USE_ZIGBUILD=false

    if command -v cross &>/dev/null && docker info &>/dev/null 2>&1; then
        USE_CROSS=true
        success "Cross-compilation: cross + Docker"
    elif command -v cargo-zigbuild &>/dev/null && command -v zig &>/dev/null; then
        USE_ZIGBUILD=true
        success "Cross-compilation: cargo-zigbuild + zig"
    else
        warn "Neither cross+Docker nor cargo-zigbuild+zig found."
        warn "Linux and Windows binaries will be SKIPPED."
        warn "Install one:"
        warn "  cargo install cross --git https://github.com/cross-rs/cross  (+ Docker)"
        warn "  cargo install cargo-zigbuild && brew install zig"
    fi
fi

success "Prerequisites OK"
echo ""

# ============================================================================
# Commit & tag
# ============================================================================

if $DO_GITHUB && ! $DRY_RUN; then
    info "Checking git state..."

    if ! git diff --quiet || ! git diff --cached --quiet; then
        warn "Uncommitted changes detected. Staging and committing version bump..."
        git add Cargo.toml Cargo.lock CHANGELOG.md \
                translations/CHANGELOG.pt-BR.md translations/CHANGELOG.es.md \
                build-native-macos.sh 2>/dev/null || true
        git commit -m "chore: bump version to ${VERSION}

Co-Authored-By: release.sh <noreply@kget.local>" || warn "Nothing new to commit"
    fi

    if git rev-parse "$TAG" &>/dev/null; then
        warn "Tag $TAG already exists — skipping tag creation"
    else
        git tag -a "$TAG" -m "KGet ${VERSION}"
        success "Created tag $TAG"
    fi

    git push origin main --tags
    success "Pushed commits and tag to origin"
    echo ""
fi

# ============================================================================
# Build helpers
# ============================================================================

mkdir -p "$RELEASE_DIR"

_build_cross() {
    local target="$1" features="$2" label="$3"
    echo -n "  Building $label... "
    local cmd="cross build --release --target $target"
    [ -n "$features" ] && cmd="$cmd --features $features"
    if $cmd >/tmp/kget_build.log 2>&1; then
        echo -e "${GREEN}OK${NC}"; return 0
    else
        echo -e "${RED}FAILED${NC}"
        grep -E "^error|cannot find|undefined reference" /tmp/kget_build.log | head -5 | sed 's/^/    /'
        return 1
    fi
}

_build_zig() {
    local target="$1" features="$2" label="$3"
    echo -n "  Building $label... "
    local cmd="cargo zigbuild --release --target $target"
    [ -n "$features" ] && cmd="$cmd --features $features"
    if $cmd >/tmp/kget_build.log 2>&1; then
        echo -e "${GREEN}OK${NC}"; return 0
    else
        echo -e "${RED}FAILED${NC}"
        tail -5 /tmp/kget_build.log | sed 's/^/    /'
        return 1
    fi
}

_copy() {
    local target="$1" dest_name="$2"
    local ext=""; [[ "$target" == *windows* ]] && ext=".exe"
    local src="target/$target/release/kget${ext}"
    local dst="$RELEASE_DIR/${dest_name}${ext}"
    [ -f "$src" ] && cp "$src" "$dst" && chmod +x "$dst" 2>/dev/null || true
    [ -f "$dst" ] && success "  → $dst" || warn "  binary not found: $src"
}

# ============================================================================
# BUILD PHASE
# ============================================================================

if $DO_BUILD; then
    sep
    info "Building macOS (native app + DMG)..."
    sep

    if command -v swift &>/dev/null; then
        if $DRY_RUN; then
            warn "DRY RUN: would run build-native-macos.sh"
        else
            bash build-native-macos.sh
            success "macOS DMG created"
        fi
    else
        warn "swift not found — skipping macOS DMG build"
    fi
    echo ""

    if $USE_CROSS || $USE_ZIGBUILD; then
        sep
        info "Adding Rust cross-compilation targets..."
        sep
        for t in x86_64-unknown-linux-gnu x86_64-unknown-linux-musl \
                  x86_64-pc-windows-gnu aarch64-unknown-linux-gnu; do
            rustup target add "$t" 2>/dev/null || true
        done
        success "Targets ready"
        echo ""

        # ---- Linux x86_64 (glibc, CLI) ----
        sep
        info "Building Linux x86_64 (glibc, CLI)..."
        sep
        if $USE_CROSS; then
            _build_cross "x86_64-unknown-linux-gnu" "" "Linux x64 CLI" \
                && _copy "x86_64-unknown-linux-gnu" "kget-${VERSION}-linux-x64"

            echo -n "  Attempting Linux x64 GUI build... "
            if cross build --release --target x86_64-unknown-linux-gnu \
                           --features gui >/tmp/kget_gui.log 2>&1; then
                echo -e "${GREEN}OK${NC}"
                _copy "x86_64-unknown-linux-gnu" "kget-${VERSION}-linux-x64-gui"
            else
                echo -e "${YELLOW}SKIPPED${NC} (X11/Wayland libs not available in cross image)"
            fi
        else
            _build_zig "x86_64-unknown-linux-gnu" "" "Linux x64 CLI" \
                && _copy "x86_64-unknown-linux-gnu" "kget-${VERSION}-linux-x64"
        fi
        echo ""

        # ---- Linux x86_64 musl (static) ----
        sep
        info "Building Linux x86_64 musl (static)..."
        sep
        if $USE_CROSS; then
            _build_cross "x86_64-unknown-linux-musl" "" "Linux x64 static" \
                && _copy "x86_64-unknown-linux-musl" "kget-${VERSION}-linux-x64-static"
        else
            _build_zig "x86_64-unknown-linux-musl" "" "Linux x64 static" \
                && _copy "x86_64-unknown-linux-musl" "kget-${VERSION}-linux-x64-static"
        fi
        echo ""

        # ---- Linux ARM64 ----
        sep
        info "Building Linux ARM64..."
        sep
        if $USE_CROSS; then
            _build_cross "aarch64-unknown-linux-gnu" "" "Linux ARM64" \
                && _copy "aarch64-unknown-linux-gnu" "kget-${VERSION}-linux-arm64"
        else
            _build_zig "aarch64-unknown-linux-gnu.2.17" "" "Linux ARM64" \
                && _copy "aarch64-unknown-linux-gnu" "kget-${VERSION}-linux-arm64"
        fi
        echo ""

        # ---- Windows x86_64 ----
        sep
        info "Building Windows x86_64..."
        sep
        if $USE_CROSS; then
            _build_cross "x86_64-pc-windows-gnu" "" "Windows x64" \
                && _copy "x86_64-pc-windows-gnu" "kget-${VERSION}-windows-x64"
        else
            _build_zig "x86_64-pc-windows-gnu" "" "Windows x64" \
                && _copy "x86_64-pc-windows-gnu" "kget-${VERSION}-windows-x64"
        fi
        echo ""
    fi

    # ---- macOS native binary (Rust CLI, no Swift) ----
    sep
    info "Building macOS CLI binary (Rust)..."
    sep
    if $DRY_RUN; then
        warn "DRY RUN: would run cargo build --release"
    else
        cargo build --release
        cp "target/release/kget" "$RELEASE_DIR/kget-${VERSION}-macos-arm64" 2>/dev/null || true
        chmod +x "$RELEASE_DIR/kget-${VERSION}-macos-arm64" 2>/dev/null || true
        success "  → $RELEASE_DIR/kget-${VERSION}-macos-arm64"
    fi
    echo ""

    # ---- SHA256 checksums ----
    sep
    info "Generating SHA256 checksums..."
    sep
    SUMS_FILE="$RELEASE_DIR/SHA256SUMS-${VERSION}.txt"

    if $DRY_RUN; then
        warn "DRY RUN: would write $SUMS_FILE"
    else
        rm -f "$SUMS_FILE"
        for f in "$RELEASE_DIR"/kget-"${VERSION}"-* \
                 "$RELEASE_DIR"/KGet-"${VERSION}"-*; do
            [ -f "$f" ] && shasum -a 256 "$f" | tee -a "$SUMS_FILE" || true
        done
        success "Checksums → $SUMS_FILE"
    fi
    echo ""
fi

# ============================================================================
# RELEASE NOTES
# ============================================================================

NOTES_FILE="$RELEASE_DIR/RELEASE_NOTES-${VERSION}.md"

if ! $DRY_RUN; then
    # Extract the section for this version from CHANGELOG.md
    awk "/^## \[${VERSION}\]/,/^## \[/" CHANGELOG.md \
        | head -n -1 \
        | sed "1s/.*/# KGet ${VERSION}/" > "$NOTES_FILE"

    # Append downloads and crates.io section
    cat >> "$NOTES_FILE" <<EOF

## Downloads

$(for f in "$RELEASE_DIR"/kget-"${VERSION}"-* "$RELEASE_DIR"/KGet-"${VERSION}"-*; do
    [ -f "$f" ] && echo "- \`$(basename "$f")\`" || true
done)
- \`SHA256SUMS-${VERSION}.txt\`: checksums for all release assets.

## Rust Library

Install from crates.io:

\`\`\`bash
cargo add Kget@${VERSION}
\`\`\`

Or install the CLI:

\`\`\`bash
cargo install Kget
\`\`\`

Use \`--features gui\` for the Rust GUI build and \`--features torrent-native\` for native torrent support.

## Checksums

See \`SHA256SUMS-${VERSION}.txt\` attached to this release.
EOF
    success "Release notes → $NOTES_FILE"
fi

# ============================================================================
# GITHUB RELEASE
# ============================================================================

if $DO_GITHUB; then
    sep
    info "Creating GitHub release ${TAG}..."
    sep

    # Collect assets
    ASSETS=()
    for f in "$RELEASE_DIR"/kget-"${VERSION}"-* \
              "$RELEASE_DIR"/KGet-"${VERSION}"-* \
              "$RELEASE_DIR"/SHA256SUMS-"${VERSION}".txt; do
        [ -f "$f" ] && ASSETS+=("$f") || true
    done

    if [ ${#ASSETS[@]} -eq 0 ]; then
        warn "No release assets found in $RELEASE_DIR — only creating the release note"
    fi

    if $DRY_RUN; then
        warn "DRY RUN: would create GitHub release $TAG with ${#ASSETS[@]} asset(s):"
        for a in "${ASSETS[@]}"; do echo "    $a"; done
    else
        # Delete existing release if present (re-release)
        gh release delete "$TAG" --yes 2>/dev/null || true

        gh release create "$TAG" \
            --title "KGet ${VERSION}" \
            --notes-file "$NOTES_FILE" \
            "${ASSETS[@]}"

        success "GitHub release created: https://github.com/davimf721/KGet/releases/tag/${TAG}"
    fi
    echo ""
fi

# ============================================================================
# CRATES.IO PUBLISH
# ============================================================================

if $DO_CRATES; then
    sep
    info "Publishing to crates.io..."
    sep

    if $DRY_RUN; then
        warn "DRY RUN: would run cargo publish"
        cargo publish --dry-run 2>&1 | tail -5
    else
        cargo publish
        success "Published Kget ${VERSION} to crates.io"
    fi
    echo ""
fi

# ============================================================================
# Summary
# ============================================================================

sep
echo -e "${GREEN}${BOLD}Release ${VERSION} complete!${NC}"
sep
echo ""
echo "Artifacts in '$RELEASE_DIR/':"
ls "$RELEASE_DIR"/ 2>/dev/null | grep "${VERSION}" | sed 's/^/  /'
echo ""
$DO_GITHUB && ! $DRY_RUN && \
    echo -e "GitHub : ${BOLD}https://github.com/davimf721/KGet/releases/tag/${TAG}${NC}"
$DO_CRATES && ! $DRY_RUN && \
    echo -e "crates : ${BOLD}https://crates.io/crates/Kget/${VERSION}${NC}"
echo ""
