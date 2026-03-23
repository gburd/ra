#!/usr/bin/env bash
set -euo pipefail

DB_NAME="${1:-imdb}"
RESULTS_DIR="benchmarks/job/results"
QUERIES_DIR="benchmarks/job/queries"

# Validate prerequisites
if ! command -v psql &>/dev/null; then
    echo "Error: psql not found on PATH."
    echo "Install PostgreSQL or add its bin directory to PATH."
    exit 1
fi

if ! psql -d "$DB_NAME" -c "SELECT 1" &>/dev/null; then
    echo "Error: Cannot connect to database '$DB_NAME'."
    echo "Run ./load_data.sh $DB_NAME first."
    exit 1
fi

if [ ! -d "$QUERIES_DIR" ] || [ -z "$(ls -A "$QUERIES_DIR"/*.sql 2>/dev/null)" ]; then
    echo "Error: No query files in $QUERIES_DIR."
    echo "Run ./download_imdb.sh first."
    exit 1
fi

mkdir -p "$RESULTS_DIR"

echo "Running JOB differential testing: Ra vs PostgreSQL"
echo "Database: $DB_NAME"
echo "======================================================="
echo ""

# Initialize results file
results_file="$RESULTS_DIR/job-ra-vs-pg.md"
{
    echo "# JOB Benchmark Results: Ra vs PostgreSQL"
    echo ""
    echo "Generated: $(date -u '+%Y-%m-%d %H:%M:%S UTC')"
    echo ""
    echo "| Query | PostgreSQL (ms) | Ra (ms) | Speedup | Status |"
    echo "|-------|-----------------|---------|---------|--------|"
} > "$results_file"

pg_total=0
pg_failures=0
query_count=0

for query_file in "$QUERIES_DIR"/*.sql; do
    [ -f "$query_file" ] || continue
    query_id=$(basename "$query_file" .sql)
    printf "Testing query %-8s " "$query_id..."

    # PostgreSQL execution time
    pg_start=$(date +%s%3N 2>/dev/null || python3 -c 'import time; print(int(time.time()*1000))')
    if psql -d "$DB_NAME" -f "$query_file" -o /dev/null 2>/dev/null; then
        pg_end=$(date +%s%3N 2>/dev/null || python3 -c 'import time; print(int(time.time()*1000))')
        pg_time=$((pg_end - pg_start))
        pg_total=$((pg_total + pg_time))
        query_count=$((query_count + 1))

        # Collect PostgreSQL EXPLAIN plan
        query_text=$(cat "$query_file")
        psql -d "$DB_NAME" \
            -c "EXPLAIN (FORMAT JSON) $query_text" \
            > "$RESULTS_DIR/pg_plan_${query_id}.json" 2>/dev/null || true

        echo "PostgreSQL: ${pg_time}ms"

        # Ra optimizer path (not yet implemented)
        ra_time="N/A"
        speedup="N/A"
        status="PG only"

        echo "| $query_id | $pg_time | $ra_time | $speedup | $status |" >> "$results_file"
    else
        echo "FAILED"
        pg_failures=$((pg_failures + 1))
        echo "| $query_id | FAIL | - | - | ERROR |" >> "$results_file"
    fi
done

# Append summary
{
    echo ""
    echo "## Summary"
    echo ""
    echo "- Queries tested: $query_count"
    echo "- Failures: $pg_failures"
    echo "- PostgreSQL total time: ${pg_total}ms"
    echo "- Ra optimizer: not yet integrated"
} >> "$results_file"

echo ""
echo "======================================================="
echo "Differential testing complete"
echo ""
echo "Queries tested:          $query_count"
echo "Failures:                $pg_failures"
echo "PostgreSQL total time:   ${pg_total}ms"
echo ""
echo "Results:          $results_file"
echo "PostgreSQL plans: $RESULTS_DIR/pg_plan_*.json"
echo ""
if [ "$pg_failures" -gt 0 ]; then
    echo "WARNING: $pg_failures query(ies) failed. Check database setup."
    exit 1
fi
