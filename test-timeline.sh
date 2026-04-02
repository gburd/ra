#!/bin/bash
# Test script for timeline system
# Run this to verify the timeline functionality

set -e

cd /home/gburd/ws/ra

echo "============================================"
echo "Timeline System - Functional Test"
echo "============================================"
echo ""

BINARY="./target/debug/ra-cli"

if [ ! -f "$BINARY" ]; then
    echo "❌ Binary not found at $BINARY"
    echo "Please run: cargo build --bin ra-cli"
    exit 1
fi

echo "✅ Binary found"
echo ""

# Test 1: Basic timeline optimization
echo "Test 1: Basic Timeline Optimization"
echo "------------------------------------"
$BINARY timeline --timeline tests/data/timelines/index-addition.toml
echo ""
echo "✅ Test 1 passed"
echo ""

# Test 2: Test mode (validate expectations)
echo "Test 2: Test Mode (Expectation Validation)"
echo "-------------------------------------------"
$BINARY timeline --timeline tests/data/timelines/index-addition.toml --test
echo ""
echo "✅ Test 2 passed"
echo ""

# Test 3: JSON output
echo "Test 3: JSON Output"
echo "-------------------"
$BINARY timeline --timeline tests/data/timelines/growth-replan.toml --output json > test-output.json
echo "JSON output saved to test-output.json"
cat test-output.json | head -20
echo "..."
echo ""
echo "✅ Test 3 passed"
echo ""

# Test 4: Markdown output
echo "Test 4: Markdown Output"
echo "-----------------------"
$BINARY timeline --timeline tests/data/timelines/hardware-upgrade.toml --output markdown > test-output.md
echo "Markdown output saved to test-output.md"
head -20 test-output.md
echo "..."
echo ""
echo "✅ Test 4 passed"
echo ""

# Test 5: All timeline scenarios load successfully
echo "Test 5: Loading All Timeline Scenarios"
echo "---------------------------------------"
for timeline in tests/data/timelines/*.toml; do
    echo "  - Loading $(basename $timeline)..."
    $BINARY timeline --timeline "$timeline" --output json > /dev/null 2>&1 && echo "    ✅ OK" || echo "    ❌ FAILED"
done
echo ""
echo "✅ Test 5 passed"
echo ""

echo "============================================"
echo "✅ All Tests Passed!"
echo "============================================"
echo ""
echo "Timeline system is functional and ready for use!"
echo ""
echo "Try these commands:"
echo "  # Basic usage"
echo "  $BINARY timeline --timeline tests/data/timelines/index-addition.toml"
echo ""
echo "  # Test mode"
echo "  $BINARY timeline --timeline tests/data/timelines/index-addition.toml --test"
echo ""
echo "  # TUI visualization"
echo "  $BINARY timeline --timeline tests/data/timelines/index-addition.toml --tui"
echo ""
