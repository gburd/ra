#!/bin/bash
#
# Validate Ra vs Postgres Benchmarking Infrastructure
# Quick validation to ensure all components are working
#

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log() { echo -e "${BLUE}[VALIDATE]${NC} $*"; }
success() { echo -e "${GREEN}[✓]${NC} $*"; }
warning() { echo -e "${YELLOW}[⚠]${NC} $*"; }
error() { echo -e "${RED}[✗]${NC} $*"; }

validate_prerequisites() {
    log "Validating prerequisites..."

    # Check PostgreSQL
    if psql --version >/dev/null 2>&1; then
        success "PostgreSQL client available"
    else
        error "PostgreSQL client not found"
        return 1
    fi

    # Check Python dependencies
    if python3 -c "import pandas, numpy, scipy" >/dev/null 2>&1; then
        success "Python analysis dependencies available"
    else
        warning "Python analysis dependencies missing (optional for validation)"
    fi

    # Check required databases
    local required_dbs=("tproc" "tproc_small" "tproc_medium")
    for db in "${required_dbs[@]}"; do
        if psql "$db" -c "SELECT 1;" >/dev/null 2>&1; then
            success "Database '$db' accessible"
        else
            error "Database '$db' not accessible"
            return 1
        fi
    done

    # Check disk space
    local available_gb=$(df . | tail -1 | awk '{print int($4/1024/1024)}')
    if [[ $available_gb -gt 5 ]]; then
        success "Sufficient disk space: ${available_gb}GB available"
    else
        warning "Limited disk space: ${available_gb}GB available (recommend >5GB)"
    fi

    return 0
}

validate_database_schema() {
    log "Validating database schemas..."

    for db in "tproc" "tproc_small" "tproc_medium"; do
        # Check required tables exist
        local table_count=$(psql "$db" -t -A -q -c "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = 'public';" | head -1 | tr -d ' \n')
        if [[ $table_count -eq 8 ]]; then
            success "Database '$db' has all 8 TPROC-H tables"
        else
            error "Database '$db' has $table_count tables (expected 8)"
            return 1
        fi

        # Check lineitem table has data
        local lineitem_count=$(psql "$db" -t -A -q -c "SELECT COUNT(*) FROM lineitem;" | head -1 | tr -d ' \n')
        if [[ $lineitem_count -gt 0 ]]; then
            success "Database '$db' lineitem table has $lineitem_count rows"
        else
            error "Database '$db' lineitem table is empty"
            return 1
        fi
    done

    return 0
}

validate_query_execution() {
    log "Validating query execution..."

    # Test simple query on each database
    for db in "tproc" "tproc_small" "tproc_medium"; do
        local result
        if result=$(psql "$db" -t -A -q -c "SELECT COUNT(*) FROM customer;" 2>/dev/null | head -1); then
            success "Query execution on '$db': $(echo $result | tr -d ' \n') customers"
        else
            error "Query execution failed on '$db'"
            return 1
        fi
    done

    # Test EXPLAIN ANALYZE functionality
    if psql "tproc_small" -c "EXPLAIN (ANALYZE, COSTS, BUFFERS) SELECT COUNT(*) FROM customer;" >/dev/null 2>&1; then
        success "EXPLAIN ANALYZE functionality working"
    else
        error "EXPLAIN ANALYZE functionality not working"
        return 1
    fi

    return 0
}

validate_benchmarking_scripts() {
    log "Validating benchmarking scripts..."

    # Check main script exists and is executable
    if [[ -x "$SCRIPT_DIR/ra-vs-postgres-comprehensive.sh" ]]; then
        success "Main benchmarking script is executable"
    else
        error "Main benchmarking script not found or not executable"
        return 1
    fi

    # Check configuration file exists
    if [[ -f "$SCRIPT_DIR/benchmark-config.toml" ]]; then
        success "Benchmark configuration file exists"
    else
        error "Benchmark configuration file not found"
        return 1
    fi

    # Validate script syntax
    if bash -n "$SCRIPT_DIR/ra-vs-postgres-comprehensive.sh"; then
        success "Benchmarking script syntax is valid"
    else
        error "Benchmarking script has syntax errors"
        return 1
    fi

    return 0
}

run_quick_benchmark_test() {
    log "Running quick benchmark test..."

    local test_results_dir="$SCRIPT_DIR/test_results/$(date +%Y%m%d_%H%M%S)"
    mkdir -p "$test_results_dir/raw_results"

    # Create a minimal test configuration
    cat > "$test_results_dir/test_config.toml" <<EOF
[general]
target_runtime_hours = 0.1
iterations_per_query = 2
query_timeout_sec = 30
parallel_jobs = 1

[databases.tproc_small]
connection_string = "postgres://localhost/tproc_small"
scale_factor = 0.1

[query_types.simple_scan]
enabled = true
iterations = 2
EOF

    # Test query execution directly
    log "Testing simple query execution..."
    local query="SELECT COUNT(*) FROM customer LIMIT 1;"

    # Test Postgres execution
    if psql "tproc_small" -q -c "$query" >/dev/null 2>&1; then
        success "Postgres query execution: OK"
    else
        error "Postgres query execution failed"
        return 1
    fi

    # Test query timing extraction
    local timing_test_file="$test_results_dir/timing_test.log"
    if psql "tproc_small" -c "\\timing on" -c "$query" > "$timing_test_file" 2>&1; then
        if grep -q "Time:" "$timing_test_file"; then
            local extracted_time=$(grep "Time:" "$timing_test_file" | awk '{print $2}' | sed 's/ms//')
            success "Query timing extraction: ${extracted_time}ms"
        else
            warning "Could not extract timing from output"
        fi
    else
        error "Query timing test failed"
        return 1
    fi

    # Cleanup test results
    rm -rf "$test_results_dir"

    return 0
}

generate_validation_report() {
    log "Generating validation report..."

    local report_file="$SCRIPT_DIR/infrastructure_validation_$(date +%Y%m%d_%H%M%S).txt"

    cat > "$report_file" <<EOF
Ra vs Postgres Benchmarking Infrastructure Validation Report
Generated: $(date)

=== System Information ===
OS: $(uname -s -r)
PostgreSQL: $(psql --version)
Python: $(python3 --version 2>/dev/null || echo "Not available")
Available Disk Space: $(df -h . | tail -1 | awk '{print $4}')

=== Database Status ===
EOF

    for db in "tproc" "tproc_small" "tproc_medium"; do
        if psql "$db" -q -c "SELECT 1;" >/dev/null 2>&1; then
            local row_count=$(psql "$db" -t -A -q -c "SELECT COUNT(*) FROM lineitem;" | head -1 | tr -d ' \n')
            echo "$db: AVAILABLE ($row_count lineitem rows)" >> "$report_file"
        else
            echo "$db: UNAVAILABLE" >> "$report_file"
        fi
    done

    cat >> "$report_file" <<EOF

=== Validation Results ===
Prerequisites: $(validate_prerequisites >/dev/null 2>&1 && echo "PASS" || echo "FAIL")
Database Schema: $(validate_database_schema >/dev/null 2>&1 && echo "PASS" || echo "FAIL")
Query Execution: $(validate_query_execution >/dev/null 2>&1 && echo "PASS" || echo "FAIL")
Benchmarking Scripts: $(validate_benchmarking_scripts >/dev/null 2>&1 && echo "PASS" || echo "FAIL")
Quick Test: $(run_quick_benchmark_test >/dev/null 2>&1 && echo "PASS" || echo "FAIL")

=== Recommendations ===
EOF

    if ! python3 -c "import pandas, numpy, scipy" >/dev/null 2>&1; then
        echo "- Install Python analysis dependencies: pip3 install pandas numpy scipy" >> "$report_file"
    fi

    local available_gb=$(df . | tail -1 | awk '{print int($4/1024/1024)}')
    if [[ $available_gb -lt 10 ]]; then
        echo "- Consider freeing disk space for comprehensive benchmarking (current: ${available_gb}GB)" >> "$report_file"
    fi

    echo "- Infrastructure validation complete" >> "$report_file"
    echo "" >> "$report_file"
    echo "Next steps:" >> "$report_file"
    echo "1. Run quick test: ./ra-vs-postgres-comprehensive.sh" >> "$report_file"
    echo "2. For full benchmark: Allow 4+ hours runtime" >> "$report_file"
    echo "3. Monitor results in: benchmarks/results/" >> "$report_file"

    success "Validation report saved: $report_file"
}

main() {
    log "Starting infrastructure validation..."

    local validation_passed=true

    if ! validate_prerequisites; then
        validation_passed=false
    fi

    if ! validate_database_schema; then
        validation_passed=false
    fi

    if ! validate_query_execution; then
        validation_passed=false
    fi

    if ! validate_benchmarking_scripts; then
        validation_passed=false
    fi

    if ! run_quick_benchmark_test; then
        validation_passed=false
    fi

    generate_validation_report

    if $validation_passed; then
        success "Infrastructure validation PASSED ✓"
        log "Ready for comprehensive Ra vs Postgres benchmarking!"
        log "Run: ./ra-vs-postgres-comprehensive.sh"
        return 0
    else
        error "Infrastructure validation FAILED ✗"
        log "Please fix the issues above before running benchmarks"
        return 1
    fi
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi