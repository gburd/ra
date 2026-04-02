#!/bin/bash
set -euo pipefail

# Start Docker services for Ra project

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_ROOT"

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

# Parse arguments
TARGET="${1:-all}"

case "$TARGET" in
    all)
        log_info "Starting all services..."
        docker compose up -d
        ;;
    core)
        log_info "Starting core services (docs, ra-web, redis, postgres-ra-extension)..."
        docker compose up -d docs ra-web redis postgres-ra-extension
        ;;
    databases)
        log_info "Starting test databases..."
        docker compose up -d postgres-15 postgres-16 mysql-8 mariadb duckdb
        ;;
    docs)
        log_info "Starting documentation site..."
        docker compose up -d docs
        ;;
    web)
        log_info "Starting ra-web..."
        docker compose up -d ra-web redis postgres-ra-extension
        ;;
    postgres)
        log_info "Starting PostgreSQL services..."
        docker compose up -d postgres-ra-extension postgres-ra-proxy
        ;;
    *)
        log_warn "Unknown target: $TARGET"
        echo "Usage: $0 [all|core|databases|docs|web|postgres]"
        exit 1
        ;;
esac

log_info "Services started! Checking health..."
sleep 5
docker compose ps

log_info ""
log_info "Access URLs:"
log_info "  Documentation: http://localhost:3000"
log_info "  Ra Web API:    http://localhost:8000"
log_info "  Ra Web Health: http://localhost:8000/health"
log_info "  PG Extension:  postgresql://ra_test:ra_test_pass@localhost:5432/ra_testdb"
log_info "  PG Proxy:      postgresql://ra_proxy:ra_proxy_pass@localhost:5433/ra_proxydb"
log_info "  Proxy API:     http://localhost:8001"
log_info ""
log_info "View logs: docker compose logs -f"
