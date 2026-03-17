#!/usr/bin/env bash
set -euo pipefail

# RA Web Explorer - Docker Launcher
# Builds and runs the web explorer in a Docker container

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$PROJECT_ROOT"

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${YELLOW}Building RA Web Explorer Docker image...${NC}"
docker build -t ra-web:latest .

echo -e "${GREEN}Build complete!${NC}"
echo ""
echo -e "${YELLOW}Starting RA Web Explorer...${NC}"
echo "Access at: http://localhost:8000"
echo ""
echo "Press Ctrl+C to stop"
echo ""

# Run container with proper signal handling
docker run --rm -it \
    --name ra-web \
    -p 8000:8000 \
    -e RUST_LOG=info \
    -v "$PROJECT_ROOT/rules:/app/rules:ro" \
    ra-web:latest
