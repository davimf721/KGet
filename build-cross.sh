#!/bin/bash
#
# KGet Cross-Compilation Script
# Build binaries for Linux and Windows from macOS
#
# Prerequisites:
#   1. Install cross: cargo install cross --git https://github.com/cross-rs/cross
#   2. Install Docker Desktop for macOS
#   3. OR use zigbuild: cargo install cargo-zigbuild && brew install zig
#

set -e

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
RELEASE_DIR="release"

echo "================================================"
echo "         KGet Cross-Compilation"
echo "         Version: $VERSION"
echo "================================================"
echo ""

mkdir -p "$RELEASE_DIR"

# ============================================================================
# Method Selection
# ============================================================================

USE_CROSS=false
USE_ZIGBUILD=false

if command -v cross &> /dev/null && docker info &> /dev/null 2>&1; then
    USE_CROSS=true
    echo -e "${GREEN}✓ Using 'cross' with Docker${NC}"
elif command -v cargo-zigbuild &> /dev/null && command -v zig &> /dev/null; then
    USE_ZIGBUILD=true
    echo -e "${GREEN}✓ Using 'cargo-zigbuild' with Zig${NC}"
else
    echo -e "${RED}Error: Neither cross+Docker nor cargo-zigbuild+zig are available.${NC}"
    echo ""
    echo "Install one of these:"
    echo ""
    echo "Option 1 - Cross (recommended for GUI):"
    echo "  cargo install cross --git https://github.com/cross-rs/cross"
    echo "  Install Docker Desktop from https://docker.com"
    echo ""
    echo "Option 2 - Zigbuild (faster, CLI only):"
    echo "  cargo install cargo-zigbuild"
    echo "  brew install zig"
    echo ""
    exit 1
fi

# ============================================================================
# Target Installation
# ============================================================================

echo ""
echo -e "${BLUE}==> Adding Rust targets...${NC}"

TARGETS=(
    "x86_64-unknown-linux-gnu"
    "x86_64-unknown-linux-musl"
    "x86_64-pc-windows-gnu"
    "aarch64-unknown-linux-gnu"
)

for target in "${TARGETS[@]}"; do
    rustup target add "$target" 2>/dev/null || true
done
echo -e "${GREEN}✓ Targets installed${NC}"

# ============================================================================
# Build Functions
# ============================================================================

build_with_cross() {
    local target="$1"
    local features="$2"
    local name="$3"
    
    echo -n "  Building $name... "
    
    local cmd="cross build --release --target $target"
    [ -n "$features" ] && cmd="$cmd --features $features"
    
    if $cmd > /tmp/cross_build.log 2>&1; then
        echo -e "${GREEN}OK${NC}"
        return 0
    else
        echo -e "${RED}FAILED${NC}"
        echo "  ERROR: "
        grep -E "^error|cannot find|undefined reference" /tmp/cross_build.log | head -5 | sed 's/^/    /'
        return 1
    fi
}

build_with_zigbuild() {
    local target="$1"
    local features="$2"
    local name="$3"
    
    echo -n "  Building $name... "
    
    # Note: GUI may not work well with zigbuild due to native dependencies
    if cargo zigbuild --release --target "$target" --features "$features" > /tmp/zig_build.log 2>&1; then
        echo -e "${GREEN}OK${NC}"
        return 0
    else
        echo -e "${RED}FAILED${NC}"
        tail -5 /tmp/zig_build.log
        return 1
    fi
}

copy_binary() {
    local target="$1"
    local output_name="$2"
    local ext=""
    
    [[ "$target" == *"windows"* ]] && ext=".exe"
    
    local src="target/$target/release/kget$ext"
    local dst="$RELEASE_DIR/$output_name$ext"
    
    if [ -f "$src" ]; then
        cp "$src" "$dst"
        chmod +x "$dst" 2>/dev/null || true
        echo "    → $dst"
    fi
}

# ============================================================================
# Linux Builds
# ============================================================================

echo ""
echo -e "${BLUE}==> Building for Linux (x86_64)...${NC}"

if [ "$USE_CROSS" = true ]; then
    # CLI build first (always works)
    build_with_cross "x86_64-unknown-linux-gnu" "" "Linux x64 (CLI)"
    copy_binary "x86_64-unknown-linux-gnu" "kget-$VERSION-linux-x64"
    
    # Try GUI build (may fail if X11 libs are missing in Docker image)
    echo -n "  Attempting GUI build... "
    if cross build --release --target x86_64-unknown-linux-gnu --features gui > /tmp/cross_gui.log 2>&1; then
        echo -e "${GREEN}OK${NC}"
        copy_binary "x86_64-unknown-linux-gnu" "kget-$VERSION-linux-x64-gui"
    else
        echo -e "${YELLOW}SKIPPED${NC} (X11 libs required)"
    fi
    
    # musl static build (CLI only, more portable)
    build_with_cross "x86_64-unknown-linux-musl" "" "Linux x64 musl (static)"
    copy_binary "x86_64-unknown-linux-musl" "kget-$VERSION-linux-x64-static"
else
    cargo zigbuild --release --target x86_64-unknown-linux-gnu 2>/dev/null || true
    copy_binary "x86_64-unknown-linux-gnu" "kget-$VERSION-linux-x64"
fi

# ============================================================================
# Linux ARM64
# ============================================================================

echo ""
echo -e "${BLUE}==> Building for Linux (ARM64)...${NC}"

if [ "$USE_CROSS" = true ]; then
    build_with_cross "aarch64-unknown-linux-gnu" "" "Linux ARM64"
    copy_binary "aarch64-unknown-linux-gnu" "kget-$VERSION-linux-arm64"
fi

# ============================================================================
# Windows Build
# ============================================================================

echo ""
echo -e "${BLUE}==> Building for Windows (x86_64)...${NC}"

if [ "$USE_CROSS" = true ]; then
    # CLI version (GUI on Windows via cross is complex)
    build_with_cross "x86_64-pc-windows-gnu" "" "Windows x64"
    copy_binary "x86_64-pc-windows-gnu" "kget-$VERSION-windows-x64"
else
    cargo zigbuild --release --target x86_64-pc-windows-gnu 2>/dev/null || true
    copy_binary "x86_64-pc-windows-gnu" "kget-$VERSION-windows-x64"
fi

# ============================================================================
# Summary
# ============================================================================

echo ""
echo "================================================"
echo "               Build Summary"
echo "================================================"
echo ""
echo "Binaries in '$RELEASE_DIR/':"
ls -la "$RELEASE_DIR"/kget-* 2>/dev/null | awk '{print "  " $9 " (" $5 " bytes)"}'
echo ""

# ============================================================================
# Notes
# ============================================================================

echo -e "${YELLOW}Notes:${NC}"
echo "• Linux GUI requires X11/Wayland libraries on the target system"
echo "• Windows GUI may require building on Windows with MSVC toolchain"
echo "• The '-static' build uses musl and has no runtime dependencies"
echo ""
echo -e "${GREEN}Cross-compilation complete!${NC}"
