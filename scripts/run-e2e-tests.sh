#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

echo "Starting E2E test setup..."

if ! command -v docker-compose &> /dev/null; then
    echo "Error: docker-compose is not installed"
    exit 1
fi

if ! command -v cargo &> /dev/null; then
    echo "Error: cargo is not installed"
    exit 1
fi

echo "Starting test databases..."
docker-compose -f docker/docker-compose.yml up -d postgres-16 mysql-8.4

sleep 5

echo "Building backend..."
cargo build --release --bin ra-web

echo "Starting backend server..."
BACKEND_PID=""
cleanup() {
    if [ -n "$BACKEND_PID" ]; then
        echo "Stopping backend server..."
        kill "$BACKEND_PID" 2>/dev/null || true
    fi
    echo "Stopping test databases..."
    docker-compose -f docker/docker-compose.yml down
}
trap cleanup EXIT

cargo run --release --bin ra-web &
BACKEND_PID=$!

echo "Waiting for backend to be ready..."
for i in {1..30}; do
    if curl -f http://localhost:8080/health &>/dev/null; then
        echo "Backend is ready"
        break
    fi
    if [ $i -eq 30 ]; then
        echo "Error: Backend failed to start"
        exit 1
    fi
    sleep 1
done

cd crates/ra-web/frontend

if [ ! -d "node_modules" ]; then
    echo "Installing frontend dependencies..."
    npm install
fi

if ! npx playwright --version &>/dev/null; then
    echo "Installing Playwright browsers..."
    npx playwright install chromium
fi

echo "Running E2E tests..."
npm run test:e2e "$@"
