#!/bin/bash
# Build script for timeline system
# Workaround for tmpdir issues

set -e

echo "Building ra-cli with timeline system..."
cd /home/gburd/ws/ra

# Build with explicit working directory
cargo build --bin ra-cli 2>&1 | tee build.log

# Check for errors
if grep -q "^error" build.log; then
    echo ""
    echo "❌ Build failed with errors:"
    grep "^error" build.log
    exit 1
else
    echo ""
    echo "✅ Build successful!"

    # Show warnings if any
    if grep -q "^warning" build.log; then
        echo ""
        echo "⚠️  Warnings (non-critical):"
        grep "^warning" build.log | head -10
    fi

    echo ""
    echo "Binary ready at: target/debug/ra-cli"
    exit 0
fi
