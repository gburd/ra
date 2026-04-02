#!/bin/bash
set -euo pipefail

# Test script for React frontend + Rocket backend integration

echo "=== Ra-Web Integration Test ==="
echo

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Track results
PASSED=0
FAILED=0

test_pass() {
    echo -e "${GREEN}✓${NC} $1"
    ((PASSED++))
}

test_fail() {
    echo -e "${RED}✗${NC} $1"
    ((FAILED++))
}

test_warn() {
    echo -e "${YELLOW}!${NC} $1"
}

# Test 1: Frontend directory exists
echo "Test 1: Frontend directory structure"
if [[ -d "crates/ra-web/frontend" ]]; then
    test_pass "Frontend directory exists"
else
    test_fail "Frontend directory missing"
fi

if [[ -f "crates/ra-web/frontend/package.json" ]]; then
    test_pass "package.json exists"
else
    test_fail "package.json missing"
fi

if [[ -f "crates/ra-web/frontend/vite.config.ts" ]]; then
    test_pass "vite.config.ts exists"
else
    test_fail "vite.config.ts missing"
fi

# Test 2: Check Vite configuration
echo
echo "Test 2: Vite configuration"
if grep -q '"outDir": "dist"' crates/ra-web/frontend/vite.config.ts; then
    test_pass "Vite output directory is dist/"
else
    test_fail "Vite output directory is not dist/"
fi

if grep -q "'/api'" crates/ra-web/frontend/vite.config.ts; then
    test_pass "Vite proxy configured for /api"
else
    test_warn "Vite proxy not found (optional for development)"
fi

# Test 3: Frontend dependencies
echo
echo "Test 3: Frontend dependencies"
if command -v node >/dev/null 2>&1; then
    NODE_VERSION=$(node --version)
    test_pass "Node.js installed: $NODE_VERSION"

    # Check if version is 22.x
    if [[ "$NODE_VERSION" =~ ^v22\. ]]; then
        test_pass "Node.js version is 22.x"
    else
        test_warn "Node.js version is not 22.x (found $NODE_VERSION)"
    fi
else
    test_fail "Node.js not installed"
fi

if command -v npm >/dev/null 2>&1; then
    NPM_VERSION=$(npm --version)
    test_pass "npm installed: $NPM_VERSION"
else
    test_fail "npm not installed"
fi

# Test 4: Frontend build output
echo
echo "Test 4: Frontend build"
if [[ -d "crates/ra-web/frontend/dist" ]]; then
    test_pass "Frontend dist directory exists"

    if [[ -f "crates/ra-web/frontend/dist/index.html" ]]; then
        test_pass "dist/index.html exists"
    else
        test_fail "dist/index.html missing"
    fi

    if [[ -d "crates/ra-web/frontend/dist/assets" ]]; then
        test_pass "dist/assets directory exists"
    else
        test_fail "dist/assets directory missing"
    fi
else
    test_warn "Frontend not built (run 'npm run build' in crates/ra-web/frontend)"
fi

# Test 5: Backend configuration
echo
echo "Test 5: Backend configuration"
if grep -q "fn frontend_dir()" crates/ra-web/src/main.rs; then
    test_pass "frontend_dir() function exists"
else
    test_fail "frontend_dir() function missing"
fi

if grep -q "FRONTEND_DIR" crates/ra-web/src/main.rs; then
    test_pass "FRONTEND_DIR environment variable support"
else
    test_fail "FRONTEND_DIR environment variable not supported"
fi

if grep -q "mount(\"/demos\"" crates/ra-web/src/main.rs; then
    test_pass "Demo pages mounted at /demos"
else
    test_fail "Demo pages not mounted at /demos"
fi

# Test 6: Static files
echo
echo "Test 6: Static files"
if [[ -d "crates/ra-web/static" ]]; then
    test_pass "Static directory exists"

    DEMO_COUNT=$(find crates/ra-web/static -name "*.html" -type f | wc -l)
    test_pass "Found $DEMO_COUNT demo HTML files"
else
    test_fail "Static directory missing"
fi

# Test 7: Dockerfile
echo
echo "Test 7: Docker configuration"
if [[ -f "Dockerfile" ]]; then
    test_pass "Dockerfile exists"

    if grep -q "frontend-build" Dockerfile; then
        test_pass "Dockerfile has frontend build stage"
    else
        test_fail "Dockerfile missing frontend build stage"
    fi

    if grep -q "FRONTEND_DIR=/app/frontend" Dockerfile; then
        test_pass "Dockerfile sets FRONTEND_DIR"
    else
        test_fail "Dockerfile doesn't set FRONTEND_DIR"
    fi

    if grep -q "STATIC_DIR=/app/static" Dockerfile; then
        test_pass "Dockerfile sets STATIC_DIR"
    else
        test_fail "Dockerfile doesn't set STATIC_DIR"
    fi
else
    test_fail "Dockerfile missing"
fi

# Test 8: Documentation
echo
echo "Test 8: Documentation"
if [[ -f "crates/ra-web/README.md" ]]; then
    test_pass "README.md exists"

    if grep -q "React" crates/ra-web/README.md; then
        test_pass "README mentions React"
    else
        test_fail "README doesn't mention React"
    fi
else
    test_fail "README.md missing"
fi

if [[ -f "crates/ra-web/INTEGRATION.md" ]]; then
    test_pass "INTEGRATION.md exists"
else
    test_fail "INTEGRATION.md missing"
fi

# Summary
echo
echo "==================================="
echo "Test Summary"
echo "==================================="
echo -e "${GREEN}Passed:${NC} $PASSED"
echo -e "${RED}Failed:${NC} $FAILED"
echo

if [[ $FAILED -eq 0 ]]; then
    echo -e "${GREEN}All tests passed!${NC}"
    exit 0
else
    echo -e "${RED}Some tests failed.${NC}"
    exit 1
fi
