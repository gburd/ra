#!/usr/bin/env bash
set -euo pipefail

# Ra Benchmark Runner
# Runs comparison benchmarks against all supported databases

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
readonly OUTPUT_DIR="${PROJECT_ROOT}/docs/benchmarks/results"
readonly TIMESTAMP=$(date +"%Y%m%d_%H%M%S")

# Colors
readonly RED='\033[0;31m'
readonly GREEN='\033[0;32m'
readonly YELLOW='\033[1;33m'
readonly BLUE='\033[0;34m'
readonly NC='\033[0m' # No Color

log_info() {
    echo -e "${BLUE}[INFO]${NC} $*"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $*"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $*"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $*"
}

setup_test_databases() {
    log_info "Setting up test databases..."

    # PostgreSQL
    if command -v psql &> /dev/null; then
        log_info "Setting up PostgreSQL test database..."
        createdb ra_benchmark_test 2>/dev/null || true
        psql -d ra_benchmark_test -c "
            CREATE TABLE IF NOT EXISTS products (
                id SERIAL PRIMARY KEY,
                name TEXT,
                description TEXT,
                price NUMERIC,
                category TEXT,
                brand_id INTEGER,
                category_id INTEGER,
                in_stock BOOLEAN,
                embedding VECTOR(3),
                search_vector TSVECTOR
            );
            CREATE INDEX IF NOT EXISTS idx_products_search ON products USING GIN(search_vector);
        " 2>/dev/null || log_warn "PostgreSQL setup failed (may not have required extensions)"
    else
        log_warn "PostgreSQL not found, skipping PostgreSQL benchmarks"
    fi

    # MySQL
    if command -v mysql &> /dev/null; then
        log_info "Setting up MySQL test database..."
        mysql -e "CREATE DATABASE IF NOT EXISTS ra_benchmark_test;" 2>/dev/null || true
        mysql ra_benchmark_test -e "
            CREATE TABLE IF NOT EXISTS products (
                id INT PRIMARY KEY AUTO_INCREMENT,
                name TEXT,
                description TEXT,
                price DECIMAL(10,2),
                category VARCHAR(255),
                brand_id INT,
                category_id INT,
                in_stock BOOLEAN
            );
        " 2>/dev/null || log_warn "MySQL setup failed"
    else
        log_warn "MySQL not found, skipping MySQL benchmarks"
    fi

    # SQLite
    log_info "Setting up SQLite test database..."
    mkdir -p "${OUTPUT_DIR}"
    sqlite3 "${OUTPUT_DIR}/ra_benchmark_test.db" "
        CREATE TABLE IF NOT EXISTS products (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT,
            description TEXT,
            price REAL,
            category TEXT,
            brand_id INTEGER,
            category_id INTEGER,
            in_stock INTEGER
        );
    " 2>/dev/null || log_warn "SQLite setup failed"

    # DuckDB
    if command -v duckdb &> /dev/null; then
        log_info "Setting up DuckDB test database..."
        duckdb "${OUTPUT_DIR}/ra_benchmark_test.duckdb" "
            CREATE TABLE IF NOT EXISTS products (
                id INTEGER PRIMARY KEY,
                name TEXT,
                description TEXT,
                price DOUBLE,
                category TEXT,
                brand_id INTEGER,
                category_id INTEGER,
                in_stock BOOLEAN
            );
        " 2>/dev/null || log_warn "DuckDB setup failed"
    else
        log_warn "DuckDB not found, skipping DuckDB benchmarks"
    fi

    log_success "Test database setup complete"
}

run_benchmarks() {
    log_info "Running benchmarks..."

    mkdir -p "${OUTPUT_DIR}"

    # Build ra-cli first
    log_info "Building ra-cli..."
    cd "${PROJECT_ROOT}"
    cargo build --release --bin ra-cli 2>&1 | grep -v "^warning:" || true

    local RA_CLI="${PROJECT_ROOT}/target/release/ra-cli"

    if [[ ! -x "${RA_CLI}" ]]; then
        log_error "ra-cli binary not found at ${RA_CLI}"
        exit 1
    fi

    # Run all benchmarks
    log_info "Running comprehensive benchmark suite..."
    "${RA_CLI}" benchmark --all \
        --format html \
        --output "${OUTPUT_DIR}/comparison_${TIMESTAMP}.html" \
        2>&1 || log_error "Benchmark run failed"

    # Also generate JSON and Markdown versions
    log_info "Generating JSON report..."
    "${RA_CLI}" benchmark --all \
        --format json \
        --output "${OUTPUT_DIR}/comparison_${TIMESTAMP}.json" \
        2>&1 || log_warn "JSON report generation failed"

    log_info "Generating Markdown report..."
    "${RA_CLI}" benchmark --all \
        --format markdown \
        --output "${OUTPUT_DIR}/comparison_${TIMESTAMP}.md" \
        2>&1 || log_warn "Markdown report generation failed"

    # Create symlinks to latest
    cd "${OUTPUT_DIR}"
    ln -sf "comparison_${TIMESTAMP}.html" latest.html
    ln -sf "comparison_${TIMESTAMP}.json" latest.json
    ln -sf "comparison_${TIMESTAMP}.md" latest.md

    log_success "Benchmarks complete!"
    log_info "Results saved to:"
    log_info "  HTML:     ${OUTPUT_DIR}/comparison_${TIMESTAMP}.html"
    log_info "  JSON:     ${OUTPUT_DIR}/comparison_${TIMESTAMP}.json"
    log_info "  Markdown: ${OUTPUT_DIR}/comparison_${TIMESTAMP}.md"
}

cleanup_test_databases() {
    log_info "Cleaning up test databases..."

    # PostgreSQL
    if command -v psql &> /dev/null; then
        dropdb ra_benchmark_test 2>/dev/null || true
    fi

    # MySQL
    if command -v mysql &> /dev/null; then
        mysql -e "DROP DATABASE IF EXISTS ra_benchmark_test;" 2>/dev/null || true
    fi

    # SQLite
    rm -f "${OUTPUT_DIR}/ra_benchmark_test.db"

    # DuckDB
    rm -f "${OUTPUT_DIR}/ra_benchmark_test.duckdb"

    log_success "Cleanup complete"
}

show_usage() {
    cat <<EOF
Usage: $(basename "$0") [OPTIONS]

Run Ra comparison benchmarks against native RDBMS implementations.

OPTIONS:
    --no-cleanup    Skip database cleanup after benchmarks
    --setup-only    Only setup test databases, don't run benchmarks
    --help          Show this help message

EXAMPLES:
    $(basename "$0")                    # Run all benchmarks
    $(basename "$0") --no-cleanup       # Keep test databases after run
    $(basename "$0") --setup-only       # Only setup test infrastructure

EOF
}

main() {
    local cleanup=true
    local setup_only=false

    while [[ $# -gt 0 ]]; do
        case $1 in
            --no-cleanup)
                cleanup=false
                shift
                ;;
            --setup-only)
                setup_only=true
                shift
                ;;
            --help|-h)
                show_usage
                exit 0
                ;;
            *)
                log_error "Unknown option: $1"
                show_usage
                exit 1
                ;;
        esac
    done

    log_info "Ra Benchmark Runner"
    log_info "==================="
    echo

    setup_test_databases

    if [[ "${setup_only}" == "true" ]]; then
        log_info "Setup complete. Exiting without running benchmarks."
        exit 0
    fi

    run_benchmarks

    if [[ "${cleanup}" == "true" ]]; then
        cleanup_test_databases
    else
        log_warn "Skipping cleanup (--no-cleanup specified)"
    fi

    log_success "All done!"
}

main "$@"
