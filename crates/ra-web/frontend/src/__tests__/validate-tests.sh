#!/usr/bin/env bash
set -euo pipefail

echo "Validating test structure..."

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."

TEST_FILES=(
  "__tests__/setup.ts"
  "__tests__/components/PlanTreeView.test.tsx"
  "__tests__/components/CostAnalysisView.test.tsx"
  "__tests__/components/WarningsView.test.tsx"
)

MISSING=0
for file in "${TEST_FILES[@]}"; do
  if [[ ! -f "$file" ]]; then
    echo "Missing: $file"
    MISSING=$((MISSING + 1))
  else
    echo "Found: $file"
  fi
done

if [[ $MISSING -gt 0 ]]; then
  echo "Error: $MISSING test files missing"
  exit 1
fi

echo "All test files present"

if [[ -f "../vitest.config.ts" ]]; then
  echo "Found vitest.config.ts"
else
  echo "Warning: vitest.config.ts not found"
fi

echo "Test structure validation complete"
