#!/usr/bin/env bash
set -euo pipefail

# RA Web Explorer - Docker Compose Launcher
# Uses docker-compose for easier local development

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$PROJECT_ROOT"

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Check if docker-compose is available
if ! command -v docker-compose &> /dev/null && ! docker compose version &> /dev/null; then
    echo "Error: docker-compose not found. Install with:"
    echo "  macOS: brew install docker-compose"
    echo "  Linux: apt-get install docker-compose"
    exit 1
fi

# Use docker compose (v2) if available, otherwise docker-compose (v1)
DOCKER_COMPOSE="docker compose"
if ! docker compose version &> /dev/null; then
    DOCKER_COMPOSE="docker-compose"
fi

echo -e "${YELLOW}Starting RA Web Explorer with Docker Compose...${NC}"
echo ""

# Build and start services
$DOCKER_COMPOSE up --build

# Note: Use Ctrl+C to stop
# To run in background: ./docker-compose-up.sh -d
# To stop background: docker compose down
