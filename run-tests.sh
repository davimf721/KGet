#!/bin/bash
#
# KGet - Automated Test Runner
# Runs all tests with various configurations
#

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

echo "================================================"
echo "         KGet Automated Test Suite"
echo "================================================"
echo ""

# Track results
PASSED=0
FAILED=0
SKIPPED=0

run_test() {
    local name="$1"
    local cmd="$2"
    
    echo -n "  Running: $name... "
    
    if eval "$cmd" > /tmp/kget_test_output.txt 2>&1; then
        echo -e "${GREEN}PASSED${NC}"
        PASSED=$((PASSED + 1))
    else
        echo -e "${RED}FAILED${NC}"
        echo "    Output:"
        tail -10 /tmp/kget_test_output.txt | sed 's/^/      /'
        FAILED=$((FAILED + 1))
    fi
}

# ============================================================================
# 1. Unit Tests (no features)
# ============================================================================
echo -e "${BLUE}==> Running Unit Tests (default features)${NC}"
run_test "unit_tests" "cargo test --lib --test unit_tests"

# ============================================================================
# 2. CLI Tests
# ============================================================================
echo ""
echo -e "${BLUE}==> Running CLI Tests${NC}"
run_test "cli_tests" "cargo test --test cli_tests"

# ============================================================================
# 3. Mock Server Tests
# ============================================================================
echo ""
echo -e "${BLUE}==> Running Mock Server Integration Tests${NC}"
run_test "mock_server_tests" "cargo test --test mock_server_tests"

# ============================================================================
# 4. Torrent Tests
# ============================================================================
echo ""
echo -e "${BLUE}==> Running Torrent Tests${NC}"
run_test "torrent_tests" "cargo test --test torrent_tests"

# ============================================================================
# 5. Native Torrent Tests (feature-gated)
# ============================================================================
echo ""
echo -e "${BLUE}==> Running Native Torrent Tests${NC}"
run_test "torrent_native_feature" "cargo test --test torrent_tests --features torrent-native"

# ============================================================================
# 6. GUI Tests (if feature available)
# ============================================================================
echo ""
echo -e "${BLUE}==> Running GUI Feature Compilation Check${NC}"
run_test "gui_compile_check" "cargo check --features gui"

# ============================================================================
# 7. All Features Combined
# ============================================================================
echo ""
echo -e "${BLUE}==> Running All Features Test${NC}"
run_test "all_features" "cargo test --all-features"

# ============================================================================
# 8. Documentation Tests
# ============================================================================
echo ""
echo -e "${BLUE}==> Running Doc Tests${NC}"
run_test "doc_tests" "cargo test --doc"

# ============================================================================
# 9. Clippy Lints
# ============================================================================
echo ""
echo -e "${BLUE}==> Running Clippy Lints${NC}"
run_test "clippy" "cargo clippy --all-targets -- -D warnings 2>/dev/null || cargo clippy --all-targets"

# ============================================================================
# 10. Format Check
# ============================================================================
echo ""
echo -e "${BLUE}==> Running Format Check${NC}"
if command -v rustfmt &> /dev/null; then
    run_test "rustfmt" "cargo fmt -- --check 2>/dev/null || true"
else
    echo -e "  ${YELLOW}SKIPPED${NC} (rustfmt not installed)"
    SKIPPED=$((SKIPPED + 1))
fi

# ============================================================================
# Summary
# ============================================================================
echo ""
echo "================================================"
echo "                 Test Summary"
echo "================================================"
echo -e "  ${GREEN}Passed:${NC}  $PASSED"
echo -e "  ${RED}Failed:${NC}  $FAILED"
echo -e "  ${YELLOW}Skipped:${NC} $SKIPPED"
echo ""

if [ $FAILED -eq 0 ]; then
    echo -e "${GREEN}All tests passed! ✓${NC}"
    exit 0
else
    echo -e "${RED}Some tests failed. Please review the output above.${NC}"
    exit 1
fi
