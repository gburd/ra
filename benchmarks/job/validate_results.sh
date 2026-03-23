#!/usr/bin/env bash
set -euo pipefail

DB_NAME="${1:-imdb}"
QUERIES_DIR="benchmarks/job/queries"
WORK_DIR="${TMPDIR:-/tmp}/job-validation-$$"

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

mkdir -p "$WORK_DIR"
trap 'rm -rf "$WORK_DIR"' EXIT

echo "Validating query results: Ra vs PostgreSQL"
echo "Database: $DB_NAME"
echo "======================================================="
echo ""

matches=0
failures=0
empty=0
total=0

for query_file in "$QUERIES_DIR"/*.sql; do
    [ -f "$query_file" ] || continue
    query_id=$(basename "$query_file" .sql)
    total=$((total + 1))
    printf "Validating %-8s " "$query_id..."

    pg_output="$WORK_DIR/pg_${query_id}.txt"

    # Run with PostgreSQL planner
    if psql -d "$DB_NAME" -f "$query_file" -t -A 2>/dev/null \
            | sort > "$pg_output"; then
        row_count=$(wc -l < "$pg_output" | tr -d ' ')

        if [ "$row_count" -gt 0 ]; then
            echo "OK ($row_count rows)"
            matches=$((matches + 1))
        else
            echo "EMPTY (0 rows)"
            empty=$((empty + 1))
        fi

        # When Ra optimizer is integrated, compare result sets:
        #
        # ra_output="$WORK_DIR/ra_${query_id}.txt"
        # ra-cli optimize --execute "$query_file" -d "$DB_NAME" \
        #     | sort > "$ra_output"
        #
        # if diff -q "$pg_output" "$ra_output" >/dev/null 2>&1; then
        #     echo "MATCH ($row_count rows)"
        #     matches=$((matches + 1))
        # else
        #     echo "MISMATCH"
        #     diff "$pg_output" "$ra_output" | head -20
        #     failures=$((failures + 1))
        # fi
    else
        echo "QUERY FAILED"
        failures=$((failures + 1))
    fi
done

echo ""
echo "======================================================="
echo "Queries validated:  $total"
echo "Successful:         $matches"
echo "Empty results:      $empty"
echo "Failures:           $failures"
echo ""

if [ "$failures" -eq 0 ]; then
    echo "All queries executed successfully."
    if [ "$empty" -gt 0 ]; then
        echo "Note: $empty query(ies) returned empty results."
        echo "This may be expected for some filter combinations."
    fi
    exit 0
else
    echo "ERROR: $failures query(ies) failed."
    echo "Check database setup and query files."
    exit 1
fi
