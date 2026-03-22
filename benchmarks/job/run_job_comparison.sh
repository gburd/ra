#!/usr/bin/env bash
set -euo pipefail

DB_NAME="${1:-imdb}"
RESULTS_DIR="benchmarks/job/results"
QUERIES_DIR="benchmarks/job/queries"

if [ ! -d "$QUERIES_DIR" ] || [ -z "$(ls -A $QUERIES_DIR/*.sql 2>/dev/null)" ]; then
    echo "Error: Queries directory not found or empty. Run ./download_imdb.sh first."
    exit 1
fi

mkdir -p "$RESULTS_DIR"

echo "Running JOB differential testing: Ra vs PostgreSQL"
echo "Database: $DB_NAME"
echo "═══════════════════════════════════════════════════"
echo ""

# Initialize results files
echo "# JOB Benchmark Results: Ra vs PostgreSQL" > "$RESULTS_DIR/job-ra-vs-pg.md"
echo "" >> "$RESULTS_DIR/job-ra-vs-pg.md"
echo "Generated: $(date)" >> "$RESULTS_DIR/job-ra-vs-pg.md"
echo "" >> "$RESULTS_DIR/job-ra-vs-pg.md"
echo "| Query | PostgreSQL (ms) | Ra (ms) | Speedup | Status |" >> "$RESULTS_DIR/job-ra-vs-pg.md"
echo "|-------|-----------------|---------|---------|--------|" >> "$RESULTS_DIR/job-ra-vs-pg.md"

pg_total=0
query_count=0

# Run each query with PostgreSQL planner
for query_file in "$QUERIES_DIR"/*.sql; do
    if [ -f "$query_file" ]; then
        query_id=$(basename "$query_file" .sql)
        echo -n "Testing query $query_id... "

        # PostgreSQL execution time
        pg_start=$(date +%s%3N)
        if psql -d "$DB_NAME" -f "$query_file" -o /dev/null 2>&1; then
            pg_end=$(date +%s%3N)
            pg_time=$((pg_end - pg_start))
            pg_total=$((pg_total + pg_time))
            query_count=$((query_count + 1))

            # Get PostgreSQL plan
            psql -d "$DB_NAME" -c "EXPLAIN (FORMAT JSON) $(cat "$query_file")" \
                > "$RESULTS_DIR/pg_plan_${query_id}.json" 2>/dev/null || true

            echo "PostgreSQL: ${pg_time}ms"

            # Ra optimizer (TODO: Implement Ra optimization path)
            ra_time="N/A"
            speedup="N/A"
            status="✓ PG"

            # Write to results file
            echo "| $query_id | $pg_time | $ra_time | $speedup | $status |" >> "$RESULTS_DIR/job-ra-vs-pg.md"
        else
            echo "FAILED"
            echo "| $query_id | - | - | - | ✗ FAILED |" >> "$RESULTS_DIR/job-ra-vs-pg.md"
        fi
    fi
done

echo ""
echo "═══════════════════════════════════════════════════"
echo "✓ Differential testing complete"
echo ""
echo "Queries tested: $query_count"
echo "PostgreSQL total time: ${pg_total}ms"
echo ""
echo "Results written to: $RESULTS_DIR/job-ra-vs-pg.md"
echo "PostgreSQL plans: $RESULTS_DIR/pg_plan_*.json"
echo ""
echo "Note: Ra optimizer integration is not yet implemented."
echo "This script currently only measures PostgreSQL baseline performance."
