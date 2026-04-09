#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

echo "Verifying E2E Test Setup"
echo "========================"
echo ""

errors=0

echo "Checking test files..."
test_files=(
  "e2e/full-workflow.spec.ts"
  "e2e/edge-cases.spec.ts"
  "e2e/api-integration.spec.ts"
  "e2e/fixtures.ts"
  "e2e/README.md"
)

for file in "${test_files[@]}"; do
  if [ -f "$file" ]; then
    lines=$(wc -l < "$file")
    echo "  ✓ $file ($lines lines)"
  else
    echo "  ✗ $file (missing)"
    ((errors++))
  fi
done

echo ""
echo "Checking configuration files..."
config_files=(
  "crates/ra-web/frontend/playwright.config.ts"
  ".github/workflows/e2e-tests.yml"
  "scripts/run-e2e-tests.sh"
)

for file in "${config_files[@]}"; do
  if [ -f "$file" ]; then
    echo "  ✓ $file"
  else
    echo "  ✗ $file (missing)"
    ((errors++))
  fi
done

echo ""
echo "Checking package.json scripts..."
if grep -q "test:e2e" crates/ra-web/frontend/package.json; then
  echo "  ✓ test:e2e script found"
else
  echo "  ✗ test:e2e script missing"
  ((errors++))
fi

if grep -q "test:e2e:ui" crates/ra-web/frontend/package.json; then
  echo "  ✓ test:e2e:ui script found"
else
  echo "  ✗ test:e2e:ui script missing"
  ((errors++))
fi

echo ""
echo "Checking Playwright dependency..."
if grep -q "@playwright/test" crates/ra-web/frontend/package.json; then
  echo "  ✓ @playwright/test dependency found"
else
  echo "  ✗ @playwright/test dependency missing"
  ((errors++))
fi

echo ""
echo "Checking TypeScript files..."
if [ -f "crates/ra-web/frontend/playwright.config.ts" ]; then
  echo "  ✓ playwright.config.ts present"
else
  echo "  ✗ playwright.config.ts missing"
  ((errors++))
fi

echo ""
echo "Test file statistics:"
echo "  Full workflow:    $(grep -c "test(" e2e/full-workflow.spec.ts 2>/dev/null || echo 0) tests"
echo "  Edge cases:       $(grep -c "test(" e2e/edge-cases.spec.ts 2>/dev/null || echo 0) tests"
echo "  API integration:  $(grep -c "test(" e2e/api-integration.spec.ts 2>/dev/null || echo 0) tests"
echo "  Total lines:      $(cat e2e/*.ts | wc -l) lines of test code"

echo ""
if [ $errors -eq 0 ]; then
  echo "✓ All E2E test files and configuration verified successfully!"
  echo ""
  echo "To run tests:"
  echo "  ./scripts/run-e2e-tests.sh"
  echo ""
  echo "Or manually:"
  echo "  cd crates/ra-web/frontend"
  echo "  npm run test:e2e"
  exit 0
else
  echo "✗ Found $errors missing or invalid files"
  exit 1
fi
