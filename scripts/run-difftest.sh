#!/usr/bin/env bash
set -euo pipefail

# Run Ra differential tests against Docker PostgreSQL instances.
#
# Usage: ./scripts/run-difftest.sh
#
# Builds the Ra extension Docker image, starts both PG services,
# waits for them to be healthy, runs the differential tests, then
# tears everything down.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
COMPOSE_FILE="$PROJECT_DIR/docker-compose.test.yml"

cleanup() {
    echo "--- Tearing down Docker services ---"
    docker compose -f "$COMPOSE_FILE" down --volumes --remove-orphans 2>/dev/null || true
}
trap cleanup EXIT

echo "--- Building Ra extension Docker image ---"
docker compose -f "$COMPOSE_FILE" build postgres-ra

echo "--- Starting PostgreSQL services ---"
docker compose -f "$COMPOSE_FILE" up -d

echo "--- Waiting for postgres-native to be healthy ---"
for i in $(seq 1 30); do
    if docker compose -f "$COMPOSE_FILE" exec -T postgres-native pg_isready -U ra_test >/dev/null 2>&1; then
        echo "postgres-native is ready"
        break
    fi
    if [ "$i" -eq 30 ]; then
        echo "ERROR: postgres-native did not become ready in time"
        docker compose -f "$COMPOSE_FILE" logs postgres-native
        exit 1
    fi
    sleep 2
done

echo "--- Waiting for postgres-ra to be healthy ---"
for i in $(seq 1 60); do
    if docker compose -f "$COMPOSE_FILE" exec -T postgres-ra pg_isready -U ra_test >/dev/null 2>&1; then
        echo "postgres-ra is ready"
        break
    fi
    if [ "$i" -eq 60 ]; then
        echo "ERROR: postgres-ra did not become ready in time"
        docker compose -f "$COMPOSE_FILE" logs postgres-ra
        exit 1
    fi
    sleep 2
done

# Give the init scripts a moment to complete
sleep 3

export RA_DATABASE_URL="host=localhost port=15433 user=ra_test password=ra_test dbname=ra_test"
export NATIVE_DATABASE_URL="host=localhost port=15432 user=ra_test password=ra_test dbname=ra_test"

echo "--- Running differential tests ---"
cargo test -p ra-difftest -- --nocapture

echo "--- All differential tests passed ---"
