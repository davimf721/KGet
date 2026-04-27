#!/bin/bash
#
# KGet Native macOS Build Script
# ===============================
# Builds the Rust backend and Swift frontend into a native macOS app
#
# Usage: ./build-native-macos.sh
#
# This creates a fully native macOS app with:
#   - SwiftUI interface
#   - Menu bar integration
#   - URL scheme handlers (kget://, magnet:)
#   - File associations (.torrent)
#   - macOS Services menu integration
#   - Native notifications
#

set -e

# Configuration
APP_NAME="KGet"
VERSION="1.6.1"
BUNDLE_ID="com.davimf721.kget"
DMG_NAME="KGet-${VERSION}-macOS-Native"
SWIFT_PROJECT_DIR="macos-app/KGet"
BUILD_DIR="build"
OUTPUT_DIR="release"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

print_step() { echo -e "${BLUE}==>${NC} ${GREEN}$1${NC}"; }
print_warning() { echo -e "${YELLOW}⚠️  $1${NC}"; }
print_error() { echo -e "${RED}❌ $1${NC}"; }
print_success() { echo -e "${GREEN}✅ $1${NC}"; }

# ============================================================================
# Check Requirements
# ============================================================================
print_step "Checking requirements..."

if ! command -v swift &> /dev/null; then
    print_error "Swift not found. Please install Xcode."
    exit 1
fi

if ! command -v cargo &> /dev/null; then
    print_error "Cargo not found. Please install Rust."
    exit 1
fi

print_success "Requirements met!"

# ============================================================================
# Close Running Instances
# ============================================================================
print_step "Closing running KGet instances..."

# Kill any running KGet processes
if pgrep -x "KGet" > /dev/null 2>&1; then
    pkill -x "KGet" 2>/dev/null || true
    print_warning "Closed running KGet app"
    sleep 1
fi

# Also kill kget-bin if running separately
if pgrep -x "kget-bin" > /dev/null 2>&1; then
    pkill -x "kget-bin" 2>/dev/null || true
    print_warning "Closed running kget-bin process"
fi

# Force close via AppleScript (more graceful)
osascript -e 'tell application "KGet" to quit' 2>/dev/null || true

print_success "All instances closed!"

# ============================================================================
# STEP 1: Build Rust binary
# ============================================================================
print_step "Building Rust backend (release mode)..."

cargo build --release --features torrent-native

if [ ! -f "target/release/kget" ]; then
    print_error "Rust build failed!"
    exit 1
fi

print_success "Rust backend built!"

# ============================================================================
# STEP 2: Build Swift app
# ============================================================================
print_step "Building Swift frontend..."

mkdir -p "$BUILD_DIR"

# Compile Swift files
swiftc \
    -O \
    -target arm64-apple-macos13.0 \
    -sdk $(xcrun --show-sdk-path) \
    -parse-as-library \
    -emit-executable \
    -o "$BUILD_DIR/KGet" \
    "$SWIFT_PROJECT_DIR/KGetApp.swift" \
    "$SWIFT_PROJECT_DIR/DownloadManager.swift" \
    "$SWIFT_PROJECT_DIR/ContentView.swift" \
    "$SWIFT_PROJECT_DIR/MenuBarView.swift" \
    "$SWIFT_PROJECT_DIR/SettingsView.swift"

if [ ! -f "$BUILD_DIR/KGet" ]; then
    print_error "Swift build failed!"
    exit 1
fi

print_success "Swift frontend built!"

# ============================================================================
# STEP 3: Create app bundle
# ============================================================================
print_step "Creating app bundle..."

# Remove old bundle completely
rm -rf "${APP_NAME}.app"

# Build in /tmp to avoid iCloud sync issues with extended attributes
TEMP_BUILD_DIR=$(mktemp -d)
TEMP_APP="${TEMP_BUILD_DIR}/${APP_NAME}.app"

# Clean extended attributes from source files
xattr -c logo.png 2>/dev/null || true
xattr -c target/release/kget 2>/dev/null || true
xattr -c "$BUILD_DIR/KGet" 2>/dev/null || true
xattr -c "$SWIFT_PROJECT_DIR/Info.plist" 2>/dev/null || true

mkdir -p "${TEMP_APP}/Contents/MacOS"
mkdir -p "${TEMP_APP}/Contents/Resources"

# Copy executables
cp "$BUILD_DIR/KGet" "${TEMP_APP}/Contents/MacOS/KGet"
cp "target/release/kget" "${TEMP_APP}/Contents/MacOS/kget-bin"

# Copy logo to Resources
if [ -f "logo.png" ]; then
    cp "logo.png" "${TEMP_APP}/Contents/Resources/logo.png"
fi

# Copy Info.plist
cp "$SWIFT_PROJECT_DIR/Info.plist" "${TEMP_APP}/Contents/Info.plist"

# Replace placeholders in Info.plist
sed -i '' "s/\$(EXECUTABLE_NAME)/KGet/g" "${TEMP_APP}/Contents/Info.plist"

print_success "App bundle created!"

# ============================================================================
# STEP 4: Create app icon
# ============================================================================
print_step "Creating app icon..."

if [ -f "logo.png" ]; then
    mkdir -p AppIcon.iconset
    
    sips -z 16 16 logo.png --out AppIcon.iconset/icon_16x16.png 2>/dev/null
    sips -z 32 32 logo.png --out AppIcon.iconset/icon_16x16@2x.png 2>/dev/null
    sips -z 32 32 logo.png --out AppIcon.iconset/icon_32x32.png 2>/dev/null
    sips -z 64 64 logo.png --out AppIcon.iconset/icon_32x32@2x.png 2>/dev/null
    sips -z 128 128 logo.png --out AppIcon.iconset/icon_128x128.png 2>/dev/null
    sips -z 256 256 logo.png --out AppIcon.iconset/icon_128x128@2x.png 2>/dev/null
    sips -z 256 256 logo.png --out AppIcon.iconset/icon_256x256.png 2>/dev/null
    sips -z 512 512 logo.png --out AppIcon.iconset/icon_256x256@2x.png 2>/dev/null
    sips -z 512 512 logo.png --out AppIcon.iconset/icon_512x512.png 2>/dev/null
    sips -z 1024 1024 logo.png --out AppIcon.iconset/icon_512x512@2x.png 2>/dev/null
    
    # Clean extended attributes from generated icons
    xattr -cr AppIcon.iconset 2>/dev/null || true
    
    iconutil -c icns AppIcon.iconset -o "${TEMP_APP}/Contents/Resources/AppIcon.icns"
    
    rm -rf AppIcon.iconset
    
    print_success "App icon created!"
else
    print_warning "logo.png not found, using default icon"
fi

# ============================================================================
# STEP 5: Code sign (if certificate available)
# ============================================================================
print_step "Code signing..."

# Clean ALL extended attributes in temp location (no iCloud interference)
xattr -cr "${TEMP_APP}" 2>/dev/null || true
find "${TEMP_APP}" -type f -exec xattr -c {} \; 2>/dev/null || true
find "${TEMP_APP}" -type d -exec xattr -c {} \; 2>/dev/null || true

# Check for Developer ID certificate
CERT_NAME=$(security find-identity -v -p codesigning | grep "Developer ID Application" | head -1 | awk -F'"' '{print $2}')

if [ -n "$CERT_NAME" ]; then
    codesign --force --deep --sign "$CERT_NAME" \
        --options runtime \
        --entitlements "$SWIFT_PROJECT_DIR/KGet.entitlements" \
        "${TEMP_APP}"
    
    print_success "App signed with: $CERT_NAME"
else
    # Ad-hoc signing for local use
    codesign --force --deep --sign - "${TEMP_APP}"
    print_warning "Ad-hoc signed (no Developer ID found)"
fi

# Move signed app to final location
mv "${TEMP_APP}" "${APP_NAME}.app"
rm -rf "${TEMP_BUILD_DIR}"

# ============================================================================
# STEP 6: Create DMG
# ============================================================================
print_step "Creating DMG installer..."

mkdir -p "$OUTPUT_DIR"
rm -f "$OUTPUT_DIR/${DMG_NAME}.dmg"

# Check if create-dmg is available
if command -v create-dmg &> /dev/null; then
    create-dmg \
        --volname "KGet ${VERSION}" \
        --volicon "${APP_NAME}.app/Contents/Resources/AppIcon.icns" \
        --window-pos 200 120 \
        --window-size 660 400 \
        --icon-size 100 \
        --icon "${APP_NAME}.app" 180 180 \
        --hide-extension "${APP_NAME}.app" \
        --app-drop-link 480 180 \
        --background "dmg_assets/background.png" \
        "$OUTPUT_DIR/${DMG_NAME}.dmg" \
        "${APP_NAME}.app"
else
    # Fallback to hdiutil
    hdiutil create -volname "KGet ${VERSION}" -srcfolder "${APP_NAME}.app" -ov -format UDZO "$OUTPUT_DIR/${DMG_NAME}.dmg"
fi

print_success "DMG created: $OUTPUT_DIR/${DMG_NAME}.dmg"

# ============================================================================
# Cleanup
# ============================================================================
rm -rf "$BUILD_DIR"

# ============================================================================
# Summary
# ============================================================================
echo ""
echo "================================================"
echo -e "${GREEN}Build Complete!${NC}"
echo "================================================"
echo ""
echo "Outputs:"
echo "  • App:  ${APP_NAME}.app"
echo "  • DMG:  $OUTPUT_DIR/${DMG_NAME}.dmg"
echo ""
echo "Features:"
echo "  ✅ Native SwiftUI interface"
echo "  ✅ Menu bar integration"
echo "  ✅ URL schemes (kget://, magnet:)"
echo "  ✅ File associations (.torrent)"
echo "  ✅ macOS Services menu"
echo "  ✅ Native notifications"
echo ""

# Show size
APP_SIZE=$(du -sh "${APP_NAME}.app" | cut -f1)
DMG_SIZE=$(du -sh "$OUTPUT_DIR/${DMG_NAME}.dmg" | cut -f1)
echo "Sizes: App = $APP_SIZE, DMG = $DMG_SIZE"
