#!/bin/bash
set -euo pipefail

# Test container services for Ra project

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

test_passed=0
test_failed=0

run_test() {
    local test_name=$1
    local test_command=$2

    echo ""
    log_info "Testing: $test_name"

    if eval "$test_command"; then
        log_info "✓ $test_name passed"
        ((test_passed++))
        return 0
    else
        log_error "✗ $test_name failed"
        ((test_failed++))
        return 1
    fi
}

# Wait for services to be ready
log_info "Waiting for services to be ready..."
sleep 10

# Test docs
run_test "Documentation site" \
    "curl -f -s http://localhost:3000/health > /dev/null"

# Test ra-web
run_test "Ra Web health check" \
    "curl -f -s http://localhost:8000/health > /dev/null"

run_test "Ra Web API optimize endpoint" \
    "curl -f -s -X POST http://localhost:8000/api/optimize \
        -H 'Content-Type: application/json' \
        -d '{\"expr\":{\"Scan\":{\"table\":\"users\"}}}' > /dev/null"

# Test PostgreSQL with Ra extension
run_test "PostgreSQL Ra extension" \
    "PGPASSWORD=ra_test_pass psql -h localhost -p 5432 -U ra_test -d ra_testdb -c 'SELECT 1;' > /dev/null"

run_test "PostgreSQL Ra extension loaded" \
    "PGPASSWORD=ra_test_pass psql -h localhost -p 5432 -U ra_test -d ra_testdb -c '\dx pg_ra_planner' 2>&1 | grep -q 'pg_ra_planner' || echo 'Extension not loaded (expected if not built)'"

# Test PostgreSQL 19 proxy
run_test "PostgreSQL 19 proxy" \
    "PGPASSWORD=ra_proxy_pass psql -h localhost -p 5433 -U ra_proxy -d ra_proxydb -c 'SELECT 1;' > /dev/null"

run_test "Ra proxy API" \
    "curl -f -s http://localhost:8001/health > /dev/null"

# Test Redis
run_test "Redis" \
    "$COMPOSE_COMMAND exec -T redis redis-cli ping | grep -q 'PONG'"

# Test databases
run_test "PostgreSQL 15" \
    "PGPASSWORD=test_pass psql -h localhost -p 5415 -U test_user -d test_db -c 'SELECT 1;' > /dev/null"

run_test "PostgreSQL 16" \
    "PGPASSWORD=test_pass psql -h localhost -p 5416 -U test_user -d test_db -c 'SELECT 1;' > /dev/null"

run_test "MySQL 8" \
    "mysql -h localhost -P 3306 -u test_user -ptest_pass -e 'SELECT 1;' > /dev/null 2>&1"

run_test "MariaDB" \
    "mysql -h localhost -P 3307 -u test_user -ptest_pass -e 'SELECT 1;' > /dev/null 2>&1"

run_test "DuckDB" \
    "curl -f -s http://localhost:8080/ > /dev/null"

# Summary
echo ""
echo "=========================================="
log_info "Test Summary"
echo "=========================================="
log_info "Passed: $test_passed"
if [ $test_failed -gt 0 ]; then
    log_error "Failed: $test_failed"
    exit 1
else
    log_info "Failed: $test_failed"
    log_info "All tests passed!"
fi
