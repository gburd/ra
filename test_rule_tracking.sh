#!/usr/bin/env bash
set -euo pipefail

# Test script to verify rule tracking functionality

echo "=== Test 1: --rules-applied without --resource-budget ==="
echo "Expected: Should default to standard budget and show rule tracking"
cargo run -p ra-cli --quiet -- optimize \
  'SELECT * FROM users WHERE age > 18 AND active = true' \
  --rules-applied

echo ""
echo "=== Test 2: --rules-evaluated without --resource-budget ==="
echo "Expected: Should default to standard budget and show evaluated rules"
cargo run -p ra-cli --quiet -- optimize \
  'SELECT * FROM users WHERE age > 18' \
  --rules-evaluated

echo ""
echo "=== Test 3: --rules-all without --resource-budget ==="
echo "Expected: Should default to standard budget and show all rule categories"
cargo run -p ra-cli --quiet -- optimize \
  'SELECT * FROM users WHERE age > 18' \
  --rules-all

echo ""
echo "=== Test 4: --rules-applied with explicit --resource-budget ==="
echo "Expected: Should use the specified budget (interactive)"
cargo run -p ra-cli --quiet -- optimize \
  'SELECT * FROM users WHERE age > 18' \
  --rules-applied \
  --resource-budget interactive

echo ""
echo "=== Test 5: No rule flags, no budget ==="
echo "Expected: Should use unbounded optimization (no tracking)"
cargo run -p ra-cli --quiet -- optimize \
  'SELECT * FROM users WHERE age > 18'

echo ""
echo "All tests completed!"
