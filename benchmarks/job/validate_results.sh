#!/usr/bin/env bash
set -euo pipefail

DB_NAME="${1:-imdb}"
QUERIES_DIR="benchmarks/job/queries"
TMP_DIR="/tmp/job-validation"

if [ ! -d "$QUERIES_DIR" ] || [ -z "$(ls -A $QUERIES_DIR/*.sql 2>/dev/null)" ]; then
    echo "Error: Queries directory not found or empty. Run ./download_imdb.sh first."
    exit 1
fi

mkdir -p "$TMP_DIR"

echo "Validating query results: Ra vs PostgreSQL"
echo "Database: $DB_NAME"
echo "═══════════════════════════════════════════════════"
echo ""

matches=0
mismatches=0
total=0

for query_file in "$QUERIES_DIR"/*.sql; do
    if [ -f "$query_file" ]; then
        query_id=$(basename "$query_file" .sql)
        total=$((total + 1))
        echo -n "Validating query $query_id... "

        # Run with PostgreSQL planner
        if psql -d "$DB_NAME" -f "$query_file" -t -A 2>/dev/null | sort > "$TMP_DIR/pg_result_${query_id}.txt"; then
            # TODO: Run with Ra optimizer when implemented
            # For now, just verify PostgreSQL returns results
            result_lines=$(wc -l < "$TMP_DIR/pg_result_${query_id}.txt" | tr -d ' ')

            if [ "$result_lines" -gt 0 ]; then
                echo "✓ ($result_lines rows)"
                matches=$((matches + 1))
            else
                echo "⚠ No results"
            fi

            # When Ra optimizer is implemented:
            # if diff -q "$TMP_DIR/pg_result_${query_id}.txt" "$TMP_DIR/ra_result_${query_id}.txt"; then
            #     echo "✓ Results match"
            #     matches=$((matches + 1))
            # else
            #     echo "✗ MISMATCH"
            #     mismatches=$((mismatches + 1))
            # fi
        else
            echo "✗ Query failed"
            mismatches=$((mismatches + 1))
        fi
    fi
done

echo ""
echo "═══════════════════════════════════════════════════"
echo "Queries validated: $total"
echo "Successful: $matches"
echo "Failed: $mismatches"

if [ $mismatches -eq 0 ]; then
    echo "✓ All queries executed successfully"
    exit 0
else
    echo "✗ $mismatches query(ies) failed"
    exit 1
fi

# Cleanup
rm -rf "$TMP_DIR"
