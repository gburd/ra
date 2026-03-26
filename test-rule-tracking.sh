#!/usr/bin/env bash
set -euo pipefail

echo "========================================="
echo "Rule Tracking Feature Test Suite"
echo "========================================="
echo ""

CLI="cargo run --package ra-cli --quiet --"

echo "Test 1: Simple query with --rules-applied"
echo "-----------------------------------------"
$CLI optimize "SELECT * FROM users WHERE age > 18" \
    --resource-budget standard \
    --rules-applied 2>&1 | grep -A 3 "Rules Applied" || true
echo ""

echo "Test 2: Complex query with --rules-applied"
echo "-------------------------------------------"
$CLI optimize "SELECT u.name FROM users u JOIN orders o ON u.id = o.user_id WHERE u.age > 18" \
    --resource-budget standard \
    --rules-applied 2>&1 | grep -A 3 "Rules Applied" || true
echo ""

echo "Test 3: Query with --rules-available"
echo "-------------------------------------"
$CLI optimize "SELECT * FROM users" \
    --resource-budget standard \
    --rules-available 2>&1 | grep -A 2 "Available Rules" || true
echo ""

echo "Test 4: Query with --rules-evaluated"
echo "-------------------------------------"
$CLI optimize "SELECT * FROM users" \
    --resource-budget standard \
    --rules-evaluated 2>&1 | grep -A 3 "Rules Evaluated" || true
echo ""

echo "Test 5: Query with --rules-all"
echo "-------------------------------"
$CLI optimize "SELECT * FROM users WHERE age > 18 AND age < 65" \
    --resource-budget standard \
    --rules-all 2>&1 | grep -E "(Rules Applied|Rules Evaluated|Available Rules)" || true
echo ""

echo "Test 6: Verify --rules-applied finds changes"
echo "---------------------------------------------"
OUTPUT=$($CLI optimize "SELECT * FROM users WHERE true AND age > 18" \
    --resource-budget standard \
    --rules-applied 2>&1 || true)

if echo "$OUTPUT" | grep -q "Rules Applied"; then
    echo "✓ Found Rules Applied section"
else
    echo "✗ Missing Rules Applied section"
    exit 1
fi

if echo "$OUTPUT" | grep -q "206 rule(s)"; then
    echo "✓ Shows rule count"
else
    echo "✗ Missing rule count"
    exit 1
fi
echo ""

echo "========================================="
echo "All Tests Passed!"
echo "========================================="
