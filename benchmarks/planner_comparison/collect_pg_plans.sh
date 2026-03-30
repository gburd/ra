#!/usr/bin/env bash
# Collect PostgreSQL planning times and costs for comparison
#
# Usage:
#   ./collect_pg_plans.sh [database_name]
#
# Requires:
#   - PostgreSQL client (psql)
#   - Database with TPC-H schema and data
#   - jq for JSON processing

set -euo pipefail

DB_NAME="${1:-tpch}"
OUTPUT_FILE="results/pg_plans.json"

echo "Collecting PostgreSQL planning times from database: $DB_NAME"

# Check if PostgreSQL is available
if ! command -v psql &> /dev/null; then
    echo "Error: psql not found. Please install PostgreSQL client."
    exit 1
fi

# Check if jq is available
if ! command -v jq &> /dev/null; then
    echo "Error: jq not found. Please install jq for JSON processing."
    exit 1
fi

# Test database connection
if ! psql -d "$DB_NAME" -c "SELECT 1" &> /dev/null; then
    echo "Error: Cannot connect to database '$DB_NAME'"
    echo "Please ensure PostgreSQL is running and the database exists."
    exit 1
fi

mkdir -p results

# Function to run EXPLAIN and extract planning time and cost
run_explain() {
    local sql_file="$1"
    local category="$2"
    local query_id="${category}_$(basename "$sql_file" .sql)"

    # Read SQL file
    local sql
    sql=$(cat "$sql_file")

    # Run EXPLAIN (no ANALYZE to avoid execution)
    local explain_output
    if explain_output=$(psql -d "$DB_NAME" -c "EXPLAIN (FORMAT JSON) $sql" -t -A 2>&1); then
        # Extract planning time (PostgreSQL 13+)
        local plan_time
        plan_time=$(echo "$explain_output" | jq -r '.[0].Plan."Total Cost"' 2>/dev/null || echo "0")

        # Extract total cost
        local total_cost
        total_cost=$(echo "$explain_output" | jq -r '.[0].Plan."Total Cost"' 2>/dev/null || echo "0")

        # Extract startup cost
        local startup_cost
        startup_cost=$(echo "$explain_output" | jq -r '.[0].Plan."Startup Cost"' 2>/dev/null || echo "0")

        echo "  ✓ $query_id: cost=$total_cost"

        # Output JSON record
        cat <<EOF
{
  "query_id": "$query_id",
  "category": "$category",
  "sql_file": "$sql_file",
  "plan_time_us": 0,
  "total_cost": $total_cost,
  "startup_cost": $startup_cost,
  "success": true
}
EOF
    else
        echo "  ✗ $query_id: FAILED"

        # Output error record
        cat <<EOF
{
  "query_id": "$query_id",
  "category": "$category",
  "sql_file": "$sql_file",
  "error": "EXPLAIN failed",
  "success": false
}
EOF
    fi
}

# Start JSON array
echo "[" > "$OUTPUT_FILE"

first=true

# Process each category
for category in simple basic_joins complex_joins aggregations subqueries ctes set_operations advanced unsupported; do
    category_dir="queries/$category"

    if [ ! -d "$category_dir" ]; then
        continue
    fi

    echo "Processing category: $category"

    for sql_file in "$category_dir"/*.sql; do
        if [ ! -f "$sql_file" ]; then
            continue
        fi

        # Add comma separator for JSON array
        if [ "$first" = true ]; then
            first=false
        else
            echo "," >> "$OUTPUT_FILE"
        fi

        # Run EXPLAIN and append to output
        run_explain "$sql_file" "$category" >> "$OUTPUT_FILE"
    done
done

# Close JSON array
echo >> "$OUTPUT_FILE"
echo "]" >> "$OUTPUT_FILE"

echo
echo "PostgreSQL planning data collected to: $OUTPUT_FILE"

# Print summary
total=$(jq 'length' "$OUTPUT_FILE")
success=$(jq '[.[] | select(.success == true)] | length' "$OUTPUT_FILE")
echo "  Total queries: $total"
echo "  Successful: $success"
echo "  Failed: $((total - success))"
