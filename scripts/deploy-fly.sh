#!/usr/bin/env bash
set -euo pipefail

# RA Web Explorer - Fly.io Deployment Script
# Deploys the web explorer to Fly.io

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$PROJECT_ROOT"

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

# Check if flyctl is installed
if ! command -v flyctl &> /dev/null; then
    echo -e "${RED}Error: flyctl not found${NC}"
    echo "Install with:"
    echo "  macOS: brew install flyctl"
    echo "  Linux: curl -L https://fly.io/install.sh | sh"
    echo "  Or visit: https://fly.io/docs/hands-on/install-flyctl/"
    exit 1
fi

# Check if logged in
if ! flyctl auth whoami &> /dev/null; then
    echo -e "${YELLOW}Not logged in to Fly.io${NC}"
    echo "Please log in first:"
    echo "  flyctl auth login"
    exit 1
fi

echo "=========================================="
echo "RA Web Explorer - Fly.io Deployment"
echo "=========================================="
echo ""

# Check if app exists
if flyctl status --app ra-explorer &> /dev/null; then
    echo -e "${YELLOW}App 'ra-explorer' already exists${NC}"
    echo "Deploying update..."
else
    echo -e "${YELLOW}Creating new Fly.io app 'ra-explorer'${NC}"
    echo ""
    echo "This will:"
    echo "  - Create a new app named 'ra-explorer'"
    echo "  - Deploy to region: iad (Washington D.C.)"
    echo "  - Allocate: 512MB RAM, 1 shared CPU"
    echo "  - Auto-scaling: 0-1 machines"
    echo ""
    read -p "Continue? (y/N) " -n 1 -r
    echo ""
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        echo "Deployment cancelled"
        exit 0
    fi

    # Create app
    flyctl apps create ra-explorer --org personal || true
fi

echo ""
echo -e "${YELLOW}Building and deploying...${NC}"
echo "This may take 5-10 minutes on first deploy"
echo ""

# Deploy to Fly.io
flyctl deploy --config fly.toml

echo ""
echo -e "${GREEN}Deployment complete!${NC}"
echo ""
echo "App URL: https://ra-explorer.fly.dev"
echo ""
echo "Useful commands:"
echo "  flyctl status          - View app status"
echo "  flyctl logs            - View logs"
echo "  flyctl ssh console     - SSH into machine"
echo "  flyctl scale show      - View scaling config"
echo "  flyctl dashboard       - Open web dashboard"
echo ""
