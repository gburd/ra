#!/bin/bash
set -euo pipefail

# Build container images for Ra project

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_ROOT"

# Detect container runtime
# shellcheck source=detect-container-runtime.sh
source "$SCRIPT_DIR/detect-container-runtime.sh"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Parse arguments
BUILD_TARGET="${1:-all}"
NO_CACHE="${2:-}"

build_service() {
    local service=$1
    local args=()

    if [ "$NO_CACHE" = "--no-cache" ]; then
        args+=("--no-cache")
    fi

    log_info "Building $service..."
    if $COMPOSE_COMMAND build ${args[@]+"${args[@]}"} "$service"; then
        log_info "Successfully built $service"
    else
        log_error "Failed to build $service"
        return 1
    fi
}

case "$BUILD_TARGET" in
    all)
        log_info "Building all container images..."
        # Use --parallel for Docker Compose, but not for podman-compose (not supported)
        if [[ "$COMPOSE_COMMAND" == *"docker"* ]] && [ "$NO_CACHE" != "--no-cache" ]; then
            $COMPOSE_COMMAND build --parallel $NO_CACHE
        else
            $COMPOSE_COMMAND build $NO_CACHE
        fi
        ;;
    docs)
        build_service docs
        ;;
    ra-web)
        build_service ra-web
        ;;
    postgres-ra-extension)
        build_service postgres-ra-extension
        ;;
    postgres-ra-proxy)
        build_service postgres-ra-proxy
        ;;
    core)
        log_info "Building core services (docs, ra-web)..."
        build_service docs
        build_service ra-web
        ;;
    postgres)
        log_info "Building PostgreSQL services..."
        build_service postgres-ra-extension
        build_service postgres-ra-proxy
        ;;
    *)
        log_error "Unknown build target: $BUILD_TARGET"
        echo "Usage: $0 [all|docs|ra-web|postgres-ra-extension|postgres-ra-proxy|core|postgres] [--no-cache]"
        exit 1
        ;;
esac

log_info "Build complete!"
