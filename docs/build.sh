#!/bin/bash
set -euo pipefail

# Documentation build script for RA
# Builds VitePress site with optional WASM integration

echo "Building RA documentation..."

# Change to docs directory
cd "$(dirname "$0")"

# Install dependencies if needed
if [ ! -d "node_modules" ]; then
    echo "Installing dependencies..."
    npm ci
fi

# Build WASM module if available
if [ -d "../crates/ra-wasm-docs" ]; then
    echo "Building WASM module for interactive docs..."
    (cd ../crates/ra-wasm-docs && wasm-pack build --target web --out-dir ../../docs/public/wasm)
fi

# Copy static assets if they exist
if [ -d "static" ]; then
    echo "Copying static assets..."
    mkdir -p .vitepress/dist
    cp -r static/* .vitepress/dist/
fi

# Build VitePress site
echo "Building VitePress site..."
npm run build

# Generate rule documentation if the tool exists
if command -v ../target/release/ra-cli &> /dev/null; then
    echo "Generating rule documentation..."
    ../target/release/ra-cli rules list --format markdown > rules/generated.md
fi

echo "Documentation build complete!"
echo "Output directory: docs/.vitepress/dist/"