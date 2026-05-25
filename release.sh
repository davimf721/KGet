#!/usr/bin/env bash
#
# KGet release orchestrator
# =========================
# Builds release artifacts, publishes crates.io, pushes git state, and creates
# a GitHub release with assets. Designed to fail early before irreversible work.
#
# Usage:
#   ./release.sh                 # full release, asks for confirmation
#   ./release.sh --yes           # full release without prompt
#   ./release.sh --dry-run       # validate and show planned actions; no build, commit, push, publish, or GitHub write
#   ./release.sh --build-only    # build artifacts only
#   ./release.sh --github-only   # create/upload GitHub release from existing artifacts
#   ./release.sh --crates-only   # publish crates.io only
#   ./release.sh --skip-tests    # skip verification tests/checks
#   ./release.sh --skip-cross    # skip Linux/Windows cross builds
#   ./release.sh --reuse-assets  # do not rebuild assets that already exist
#   ./release.sh --force-tag     # move existing tag to HEAD
#
# Required for full release:
#   cargo, rustup, git, gh authenticated, crates.io token/login
#   macOS app: swift, xcrun, codesign, hdiutil
#   cross-platform binaries: cross+Docker or cargo-zigbuild+zig
#   Homebrew tap: SSH access to git@github.com:davimf721/homebrew-kget.git
#
# Optional environment:
#   KGET_RELEASE_INCLUDE_UNTRACKED=true  # include untracked files in the release commit
#   KGET_STALE_VERSION_REGEX='...'       # extra visible stale-version guard

set -euo pipefail

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
sep()     { echo -e "${BLUE}$(printf '─%.0s' {1..64})${NC}"; }

run() {
    if $DRY_RUN; then
        echo "DRY RUN: $*"
    else
        "$@"
    fi
}

DO_BUILD=true
DO_GITHUB=true
DO_CRATES=true
DO_CROSS=true
DO_TESTS=true
DRY_RUN=false
YES=false
FORCE_TAG=false
REUSE_ASSETS=false
REMOTE="${KGET_RELEASE_REMOTE:-origin}"
BRANCH="${KGET_RELEASE_BRANCH:-main}"
RELEASE_DIR="release"
INCLUDE_UNTRACKED="${KGET_RELEASE_INCLUDE_UNTRACKED:-false}"

for arg in "$@"; do
    case "$arg" in
        --yes|-y)       YES=true ;;
        --dry-run)      DRY_RUN=true ;;
        --build-only)   DO_GITHUB=false; DO_CRATES=false ;;
        --github-only)  DO_BUILD=false; DO_CRATES=false; DO_TESTS=false ;;
        --crates-only)  DO_BUILD=false; DO_GITHUB=false; DO_CROSS=false ;;
        --skip-tests)   DO_TESTS=false ;;
        --skip-cross)   DO_CROSS=false ;;
        --reuse-assets) REUSE_ASSETS=true ;;
        --force-tag)    FORCE_TAG=true ;;
        --help|-h)
            sed -n '/^# Usage:/,/^# Required/p' "$0" | sed 's/^# \{0,1\}//'
            exit 0 ;;
        *) die "Unknown argument: $arg. Use --help." ;;
    esac
done

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
[ -n "$VERSION" ] || die "Could not read version from Cargo.toml"
TAG="v${VERSION}"
NOTES_FILE="$RELEASE_DIR/RELEASE_NOTES-${VERSION}.md"
SUMS_FILE="$RELEASE_DIR/SHA256SUMS-${VERSION}.txt"

MAC_DMG="$RELEASE_DIR/KGet-${VERSION}-macOS-Native.dmg"
MAC_CLI="$RELEASE_DIR/kget-${VERSION}-macos-arm64"
LINUX_X64="$RELEASE_DIR/kget-${VERSION}-linux-x64"
LINUX_STATIC="$RELEASE_DIR/kget-${VERSION}-linux-x64-static"
LINUX_ARM64="$RELEASE_DIR/kget-${VERSION}-linux-arm64"
WINDOWS_X64="$RELEASE_DIR/kget-${VERSION}-windows-x64.exe"

USE_CROSS=false
USE_ZIGBUILD=false
STALE_VERSION_REGEX="${KGET_STALE_VERSION_REGEX:-}"
if [ -z "$STALE_VERSION_REGEX" ] && [ "$VERSION" = "1.6.3" ]; then
    STALE_VERSION_REGEX="v1\\.6\\.2|Version 1\\.6\\.2|KGet v1\\.6\\.2|1\\.6\\.1"
fi

print_plan() {
    echo ""
    echo -e "${BOLD}KGet Release ${VERSION}${NC}"
    sep
    echo "  Tag         : $TAG"
    echo "  Branch      : $BRANCH"
    echo "  Remote      : $REMOTE"
    echo "  Build       : $DO_BUILD"
    echo "  Cross       : $DO_CROSS"
    echo "  Tests       : $DO_TESTS"
    echo "  crates.io   : $DO_CRATES"
    echo "  GitHub      : $DO_GITHUB"
    echo "  Reuse assets: $REUSE_ASSETS"
    echo "  Untracked   : $INCLUDE_UNTRACKED"
    echo "  Dry run     : $DRY_RUN"
    sep
    echo ""
}

require_cmd() {
    command -v "$1" >/dev/null 2>&1 || die "$1 not found"
}

detect_cross_tool() {
    if ! $DO_BUILD || ! $DO_CROSS; then
        return
    fi

    if command -v cross >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
        USE_CROSS=true
        success "Cross tool: cross + Docker"
    elif command -v cargo-zigbuild >/dev/null 2>&1 && command -v zig >/dev/null 2>&1; then
        USE_ZIGBUILD=true
        success "Cross tool: cargo-zigbuild + zig"
    else
        die "Need cross+Docker or cargo-zigbuild+zig for Linux/Windows artifacts. Use --skip-cross only for non-full releases."
    fi
}

preflight() {
    info "Running preflight checks..."
    require_cmd git
    require_cmd cargo
    require_cmd rustup

    [ -f Cargo.toml ] || die "Run this script from the repository root"
    git rev-parse --is-inside-work-tree >/dev/null || die "Not inside a git repository"

    CURRENT_BRANCH=$(git branch --show-current)
    [ "$CURRENT_BRANCH" = "$BRANCH" ] || die "Current branch is '$CURRENT_BRANCH', expected '$BRANCH'"

    if $DO_GITHUB; then
        require_cmd gh
        if ! $DRY_RUN; then
            gh auth status >/dev/null || die "gh is not authenticated"
        fi
    fi

    if $DO_CRATES && ! $DRY_RUN; then
        cargo login --help >/dev/null || die "cargo login unavailable"
    fi

    if $DO_BUILD; then
        require_cmd shasum
        require_cmd tar
        require_cmd zip
        if [[ "$(uname -s)" == "Darwin" ]]; then
            require_cmd swift
            require_cmd xcrun
            require_cmd codesign
            require_cmd hdiutil
        else
            die "macOS app build requires running the full release on macOS"
        fi
    fi

    detect_cross_tool

    if ! grep -q "^## \\[${VERSION}\\]" CHANGELOG.md; then
        die "CHANGELOG.md has no section for ${VERSION}"
    fi

    if [ -n "$STALE_VERSION_REGEX" ] && rg -n "$STALE_VERSION_REGEX" \
        README.md LIB.md Cargo.toml macos-app/KGet macos-app/ShareExtension \
        translations/README.pt-BR.md translations/README.es.md \
        translations/LIB.pt-br.md translations/LIB.es.md >/tmp/kget_stale_versions.log 2>&1; then
        cat /tmp/kget_stale_versions.log
        die "Found stale visible version references"
    fi

    if $DO_GITHUB && ! $DRY_RUN; then
        git fetch "$REMOTE" "$BRANCH" --tags
        git push --dry-run "$REMOTE" "$BRANCH" >/dev/null || die "Dry-run push failed"
    fi

    success "Preflight OK"
}

confirm_release() {
    if $DRY_RUN || $YES || { ! $DO_CRATES && ! $DO_GITHUB; }; then
        return
    fi

    echo -n "Proceed with publishing KGet ${VERSION}? Type '${VERSION}' to continue: "
    read -r answer
    [ "$answer" = "$VERSION" ] || die "Aborted"
}

commit_release_state() {
    if ! $DO_GITHUB; then
        warn "Skipping release commit"
        return
    fi

    info "Committing release state..."
    local untracked
    untracked=$(git ls-files --others --exclude-standard)

    if $DRY_RUN; then
        git status --short
        if [ -n "$untracked" ] && [ "$INCLUDE_UNTRACKED" != "true" ]; then
            warn "DRY RUN: untracked files would block release commit unless ignored or KGET_RELEASE_INCLUDE_UNTRACKED=true is set"
        fi
        warn "DRY RUN: would stage tracked changes and commit if needed"
        return
    fi

    if [ -n "$untracked" ] && [ "$INCLUDE_UNTRACKED" != "true" ]; then
        echo "$untracked"
        die "Untracked files present. Ignore them, commit them separately, or set KGET_RELEASE_INCLUDE_UNTRACKED=true."
    fi

    if [ "$INCLUDE_UNTRACKED" = "true" ]; then
        git add -A
    else
        git add -u
    fi
    if git diff --cached --quiet; then
        success "No changes to commit"
    else
        git commit -m "chore: release ${VERSION}"
        success "Committed release ${VERSION}"
    fi
}

prepare_tag() {
    if ! $DO_GITHUB; then
        warn "Skipping git tag"
        return
    fi

    info "Preparing tag ${TAG}..."
    local head_commit
    head_commit=$(git rev-parse HEAD)

    if git rev-parse "$TAG" >/dev/null 2>&1; then
        local tag_commit
        tag_commit=$(git rev-list -n 1 "$TAG")
        if [ "$tag_commit" = "$head_commit" ]; then
            warn "Tag ${TAG} already exists at HEAD"
        elif $FORCE_TAG; then
            run git tag -fa "$TAG" -m "KGet ${VERSION}"
        else
            die "Tag ${TAG} exists at ${tag_commit}, not HEAD ${head_commit}. Use --force-tag."
        fi
    else
        run git tag -a "$TAG" -m "KGet ${VERSION}"
    fi
}

verify() {
    if ! $DO_TESTS; then
        warn "Skipping tests/checks"
        return
    fi

    info "Running focused verification..."
    export CARGO_INCREMENTAL=0
    cargo check --locked
    cargo check --locked --features gui
    cargo test --locked --test unit_tests --test cli_tests --test torrent_tests
    cargo test --locked --test mock_server_tests
    cargo publish --dry-run
    success "Verification OK"
}

need_asset() {
    local path="$1"
    ! $REUSE_ASSETS || [ ! -s "$path" ]
}

build_macos() {
    info "Building macOS release artifacts..."
    mkdir -p "$RELEASE_DIR"

    if $DRY_RUN; then
        warn "DRY RUN: would build $MAC_CLI and $MAC_DMG"
        return
    fi

    if need_asset "$MAC_DMG" || need_asset "$MAC_CLI"; then
        cargo build --release --features torrent-native
        cp target/release/kget "$MAC_CLI"
        chmod +x "$MAC_CLI"
        KGET_SKIP_RUST_BUILD=true bash build-native-macos.sh
    else
        warn "Reusing existing macOS artifacts"
    fi

    [ -s "$MAC_DMG" ] || die "Missing macOS DMG: $MAC_DMG"
    [ -s "$MAC_CLI" ] || die "Missing macOS CLI: $MAC_CLI"
    success "macOS artifacts ready"
}

build_target() {
    local target="$1"
    local output="$2"
    local label="$3"
    local ext=""
    [[ "$target" == *windows* ]] && ext=".exe"

    if $REUSE_ASSETS && [ -s "$output" ]; then
        warn "Reusing $output"
        return
    fi

    if $DRY_RUN; then
        warn "DRY RUN: would build ${label} into $output"
        return
    fi

    info "Building ${label}..."
    if $USE_CROSS; then
        CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}" cross build --release --target "$target"
    else
        CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}" cargo zigbuild --release --target "$target"
    fi

    local src="target/${target}/release/kget${ext}"
    [ -s "$src" ] || die "Build finished but binary missing: $src"
    cp "$src" "$output"
    chmod +x "$output" 2>/dev/null || true
    success "$output"
}

build_cross_artifacts() {
    if ! $DO_CROSS; then
        warn "Skipping Linux/Windows builds"
        return
    fi

    info "Installing Rust targets..."
    if $DRY_RUN; then
        warn "DRY RUN: would install Rust cross targets and build Linux/Windows binaries"
        return
    fi

    rustup target add x86_64-unknown-linux-gnu x86_64-unknown-linux-musl \
        aarch64-unknown-linux-gnu x86_64-pc-windows-gnu >/dev/null

    build_target "x86_64-unknown-linux-gnu" "$LINUX_X64" "Linux x64"
    build_target "x86_64-unknown-linux-musl" "$LINUX_STATIC" "Linux x64 static"
    build_target "aarch64-unknown-linux-gnu" "$LINUX_ARM64" "Linux ARM64"
    build_target "x86_64-pc-windows-gnu" "$WINDOWS_X64" "Windows x64"
}

package_assets() {
    info "Packaging assets..."

    if $DRY_RUN; then
        warn "DRY RUN: would package archives and write $SUMS_FILE"
        return
    fi

    local mac_cli_archive="$RELEASE_DIR/kget-${VERSION}-macos-arm64.tar.gz"
    local linux_x64_archive="$RELEASE_DIR/kget-${VERSION}-linux-x64.tar.gz"
    local linux_static_archive="$RELEASE_DIR/kget-${VERSION}-linux-x64-static.tar.gz"
    local linux_arm64_archive="$RELEASE_DIR/kget-${VERSION}-linux-arm64.tar.gz"
    local windows_archive="$RELEASE_DIR/kget-${VERSION}-windows-x64.zip"

    tar -czf "$mac_cli_archive" -C "$RELEASE_DIR" "$(basename "$MAC_CLI")"
    if $DO_CROSS; then
        tar -czf "$linux_x64_archive" -C "$RELEASE_DIR" "$(basename "$LINUX_X64")"
        tar -czf "$linux_static_archive" -C "$RELEASE_DIR" "$(basename "$LINUX_STATIC")"
        tar -czf "$linux_arm64_archive" -C "$RELEASE_DIR" "$(basename "$LINUX_ARM64")"
        (cd "$RELEASE_DIR" && zip -q "$(basename "$windows_archive")" "$(basename "$WINDOWS_X64")")
    fi

    rm -f "$SUMS_FILE"
    for f in "$RELEASE_DIR"/kget-"${VERSION}"-* \
             "$RELEASE_DIR"/KGet-"${VERSION}"-*; do
        [ -f "$f" ] && shasum -a 256 "$f" >> "$SUMS_FILE"
    done

    [ -s "$SUMS_FILE" ] || die "No checksums generated"
    success "Checksums: $SUMS_FILE"
}

build_assets() {
    if ! $DO_BUILD; then
        warn "Skipping build phase"
        return
    fi

    mkdir -p "$RELEASE_DIR"
    build_macos
    build_cross_artifacts
    package_assets
}

make_release_notes() {
    info "Generating release notes..."
    mkdir -p "$RELEASE_DIR"

    if $DRY_RUN; then
        warn "DRY RUN: would generate $NOTES_FILE"
        return
    fi

    awk -v version="$VERSION" '
        /^## \[/ {
            if ($0 ~ "^## \\[" version "\\]") {
                in_section = 1
            } else if (in_section) {
                exit
            }
        }
        in_section { print }
    ' CHANGELOG.md | sed "1s/.*/# KGet ${VERSION}/" > "$NOTES_FILE"

    [ -s "$NOTES_FILE" ] || die "Could not extract notes for ${VERSION}"

    {
        echo ""
        echo "## Downloads"
        for f in "$RELEASE_DIR"/KGet-"${VERSION}"-* "$RELEASE_DIR"/kget-"${VERSION}"-*; do
            [ -f "$f" ] && echo "- \`$(basename "$f")\`"
        done
        echo "- \`SHA256SUMS-${VERSION}.txt\`: checksums for all release assets."
        echo ""
        echo "## Install"
        echo ""
        echo "**Homebrew (macOS / Linux):**"
        echo '```bash'
        echo "brew tap davimf721/kget"
        echo "brew install kget"
        echo '```'
        echo ""
        echo "**Rust / crates.io:**"
        echo '```bash'
        echo "cargo install Kget --version ${VERSION}"
        echo '```'
    } >> "$NOTES_FILE"

    success "$NOTES_FILE"
}

publish_crates() {
    if ! $DO_CRATES; then
        warn "Skipping crates.io publish"
        return
    fi

    info "Publishing Kget ${VERSION} to crates.io..."
    if $DRY_RUN; then
        cargo publish --dry-run
    else
        cargo publish
    fi
    success "crates.io publish complete"
}

push_git() {
    if ! $DO_GITHUB; then
        warn "Skipping git push"
        return
    fi

    info "Pushing commit and tag..."
    run git push "$REMOTE" "$BRANCH"
    if $FORCE_TAG; then
        run git push "$REMOTE" "refs/tags/${TAG}" --force
    else
        run git push "$REMOTE" "refs/tags/${TAG}"
    fi
    success "Git push complete"
}

create_github_release() {
    if ! $DO_GITHUB; then
        warn "Skipping GitHub release"
        return
    fi

    info "Creating GitHub release ${TAG}..."

    if $DRY_RUN; then
        warn "DRY RUN: would create release $TAG and upload release assets"
        return
    fi

    local assets=()
    for f in "$RELEASE_DIR"/KGet-"${VERSION}"-* \
             "$RELEASE_DIR"/kget-"${VERSION}"-* \
             "$SUMS_FILE"; do
        [ -f "$f" ] && assets+=("$f")
    done

    [ ${#assets[@]} -gt 0 ] || die "No release assets found"

    if gh release view "$TAG" >/dev/null 2>&1; then
        if $FORCE_TAG; then
            warn "GitHub release exists; deleting and recreating ${TAG}"
            gh release delete "$TAG" --yes
        else
            die "GitHub release ${TAG} already exists. Use --force-tag to replace it."
        fi
    fi

    gh release create "$TAG" \
        --title "KGet ${VERSION}" \
        --notes-file "$NOTES_FILE" \
        "${assets[@]}"

    success "GitHub release: https://github.com/davimf721/KGet/releases/tag/${TAG}"
}

update_homebrew_formula() {
    if ! $DO_GITHUB; then
        warn "Skipping Homebrew formula update (--github disabled)"
        return
    fi

    if $DRY_RUN; then
        warn "DRY RUN: would fetch tarball SHA256, update Formula/kget.rb, and push to homebrew-kget tap"
        return
    fi

    info "Updating Formula/kget.rb for ${TAG}..."
    local tarball_url="https://github.com/davimf721/KGet/archive/refs/tags/${TAG}.tar.gz"

    # Retry up to 10 times — GitHub CDN may take a few seconds after tag push
    local sha=""
    for i in $(seq 1 10); do
        sha=$(curl -fsSL "$tarball_url" 2>/dev/null | shasum -a 256 | awk '{print $1}')
        [ -n "$sha" ] && break
        warn "Attempt $i: tarball not yet available, retrying in 5s…"
        sleep 5
    done

    if [ -z "$sha" ]; then
        warn "Could not compute tarball SHA256 — update Formula/kget.rb manually"
        return
    fi

    # Update tarball URL to new version tag
    sed -i.bak -E "s|refs/tags/v[0-9]+\.[0-9]+\.[0-9]+\.tar\.gz|refs/tags/${TAG}.tar.gz|g" Formula/kget.rb
    # Update bottle root_url to new version tag
    sed -i.bak -E "s|releases/download/v[0-9]+\.[0-9]+\.[0-9]+\"|releases/download/${TAG}\"|g" Formula/kget.rb
    # Update sha256 — handles both PLACEHOLDER and an existing 64-char hex hash
    sed -i.bak -E "s/sha256 \"(PLACEHOLDER_SHA256|[a-f0-9]{64})\"/sha256 \"${sha}\"/" Formula/kget.rb
    rm -f Formula/kget.rb.bak

    git add Formula/kget.rb
    if ! git diff --cached --quiet; then
        git commit -m "chore: update Homebrew formula to ${VERSION}"
        git push "$REMOTE" "$BRANCH"
        success "Formula/kget.rb committed (SHA256: ${sha})"
    else
        success "Formula/kget.rb already up to date"
    fi

    push_formula_to_tap
}

push_formula_to_tap() {
    local tap_repo="git@github.com:davimf721/homebrew-kget.git"
    local tap_dir
    tap_dir=$(mktemp -d)

    info "Pushing formula to davimf721/homebrew-kget tap..."

    if ! git clone --depth=1 "$tap_repo" "$tap_dir" 2>/dev/null; then
        warn "Could not clone homebrew-kget tap — copy Formula/kget.rb to the tap manually"
        rm -rf "$tap_dir"
        return
    fi

    # Tap stores the formula at root level as kget.rb
    cp Formula/kget.rb "$tap_dir/kget.rb"

    git -C "$tap_dir" add kget.rb
    if git -C "$tap_dir" diff --cached --quiet; then
        success "homebrew-kget tap already up to date"
        rm -rf "$tap_dir"
        return
    fi

    git -C "$tap_dir" \
        -c "user.name=$(git config user.name)" \
        -c "user.email=$(git config user.email)" \
        commit -m "chore: release ${VERSION}"
    git -C "$tap_dir" push origin HEAD
    success "homebrew-kget tap updated: https://github.com/davimf721/homebrew-kget"

    rm -rf "$tap_dir"
}

final_push_verify() {
    if ! $DO_GITHUB || $DRY_RUN; then
        return
    fi

    info "Final remote verification..."
    git fetch "$REMOTE" "$BRANCH" --tags >/dev/null
    local local_head remote_head
    local_head=$(git rev-parse HEAD)
    remote_head=$(git rev-parse "${REMOTE}/${BRANCH}")
    [ "$local_head" = "$remote_head" ] || die "Remote ${REMOTE}/${BRANCH} is not at local HEAD"
    git ls-remote --tags "$REMOTE" "$TAG" | grep -q "$TAG" || die "Remote tag ${TAG} missing"
    success "Remote branch and tag verified"
}

print_summary() {
    sep
    echo -e "${GREEN}${BOLD}KGet ${VERSION} release flow complete${NC}"
    sep
    echo "Artifacts:"
    if [ -d "$RELEASE_DIR" ]; then
        local found=false
        while IFS= read -r -d '' artifact; do
            found=true
            ls -lh "$artifact" | awk '{print "  " $9 " (" $5 ")"}'
        done < <(find "$RELEASE_DIR" -maxdepth 1 -type f -name "*${VERSION}*" -print0)
        $found || echo "  none yet"
    else
        echo "  none yet"
    fi
    echo ""
    if $DO_GITHUB && ! $DRY_RUN; then
        echo "GitHub : https://github.com/davimf721/KGet/releases/tag/${TAG}"
        echo "Tap    : https://github.com/davimf721/homebrew-kget"
    fi
    if $DO_CRATES && ! $DRY_RUN; then
        echo "crates : https://crates.io/crates/Kget/${VERSION}"
    fi
}

print_plan
preflight
confirm_release
commit_release_state
prepare_tag
verify
build_assets
make_release_notes
push_git
publish_crates
create_github_release
update_homebrew_formula
final_push_verify
print_summary
