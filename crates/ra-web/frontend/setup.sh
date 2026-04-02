#!/bin/bash
set -euo pipefail

echo "🚀 Setting up RA Web Frontend..."

if ! command -v node &> /dev/null; then
    echo "❌ Node.js not found. Please install Node.js 20+ first."
    exit 1
fi

echo "📦 Installing dependencies..."
npm install

echo "✅ Setup complete!"
echo ""
echo "To start development:"
echo "  npm run dev"
echo ""
echo "To build for production:"
echo "  npm run build"
echo ""
echo "Backend must be running at http://localhost:8000"
echo "  cargo run --bin ra-web"
