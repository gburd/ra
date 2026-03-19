#!/usr/bin/env bash
set -euo pipefail

# RA Web Explorer - Compose Launcher
# Uses docker-compose/podman-compose for local development

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$PROJECT_ROOT"

# Detect container runtime
# shellcheck source=detect-container-runtime.sh
source "$SCRIPT_DIR/detect-container-runtime.sh"

# Colors for output
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${YELLOW}Starting RA Web Explorer with ${COMPOSE_COMMAND}...${NC}"
echo ""

# Pass through all arguments (e.g. -d for detached mode)
$COMPOSE_COMMAND up --build "$@"

# Note: Use Ctrl+C to stop
# To run in background: ./docker-compose-up.sh -d
# To stop background: $COMPOSE_COMMAND down
