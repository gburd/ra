#!/bin/bash
set -euo pipefail

# Build script for Ra WASM documentation module

echo "Building Ra WASM documentation module..."

# Check if wasm-pack is installed
if ! command -v wasm-pack &> /dev/null; then
    echo "Error: wasm-pack is not installed"
    echo "Please install it with: cargo install wasm-pack"
    exit 1
fi

# Check if wasm32-unknown-unknown target is installed
if ! rustup target list --installed | grep -q "wasm32-unknown-unknown"; then
    echo "Installing wasm32-unknown-unknown target..."
    rustup target add wasm32-unknown-unknown
fi

# Build the WASM module
echo "Building WASM module..."
cd ../crates/ra-wasm-docs
wasm-pack build --target web --out-dir ../../docs/static/wasm --release

# Optimize the WASM file if wasm-opt is available
if command -v wasm-opt &> /dev/null; then
    echo "Optimizing WASM module..."
    wasm-opt -Oz \
        ../../docs/static/wasm/ra_wasm_docs_bg.wasm \
        -o ../../docs/static/wasm/ra_wasm_docs_bg.wasm
else
    echo "Warning: wasm-opt not found, skipping optimization"
    echo "Install with: npm install -g wasm-opt"
fi

echo "WASM module built successfully!"
echo "Output: docs/static/wasm/"
echo ""
echo "To use in documentation:"
echo "1. Include the CSS: <link rel=\"stylesheet\" href=\"/static/css/ra-interactive.css\">"
echo "2. Include the JS: <script type=\"module\" src=\"/static/js/ra-interactive.js\"></script>"
echo "3. Use \`\`\`sql-interactive code blocks in your markdown"