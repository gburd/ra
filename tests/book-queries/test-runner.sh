#!/usr/bin/env bash
# Test runner for book SQL queries
# Tests each query against Ra's parser and optimizer

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Counters
total_queries=0
parse_success=0
parse_failure=0
optimize_success=0
optimize_failure=0

# Output files
results_dir="/Users/gregburd/src/ra/tests/book-queries/results"
mkdir -p "$results_dir"
results_file="$results_dir/RESULTS.md"
failures_file="$results_dir/FAILURES.md"

# Initialize results files
cat > "$results_file" <<EOF
# SQL Query Test Results
Generated: $(date)

## Summary
EOF

cat > "$failures_file" <<EOF
# SQL Query Failures
Generated: $(date)

## Parse Failures
EOF

# Function to test a single query
test_query() {
    local file="$1"
    local query="$2"
    local query_num="$3"

    total_queries=$((total_queries + 1))

    echo "Testing query $query_num from $file..."

    # Test parse
    if cargo run --bin ra-cli -- parse "$query" > "$results_dir/parse_${query_num}.txt" 2>&1; then
        parse_success=$((parse_success + 1))
        echo -e "${GREEN}✓${NC} Parse succeeded"

        # Test optimize
        if cargo run --bin ra-cli -- optimize "$query" > "$results_dir/optimize_${query_num}.txt" 2>&1; then
            optimize_success=$((optimize_success + 1))
            echo -e "${GREEN}✓${NC} Optimize succeeded"
        else
            optimize_failure=$((optimize_failure + 1))
            echo -e "${RED}✗${NC} Optimize failed"

            cat >> "$failures_file" <<EOF

### Query $query_num (${file})
**Query:**
\`\`\`sql
$query
\`\`\`

**Optimize Error:**
\`\`\`
$(cat "$results_dir/optimize_${query_num}.txt")
\`\`\`

EOF
        fi
    else
        parse_failure=$((parse_failure + 1))
        optimize_failure=$((optimize_failure + 1))
        echo -e "${RED}✗${NC} Parse failed"

        cat >> "$failures_file" <<EOF

### Query $query_num (${file})
**Query:**
\`\`\`sql
$query
\`\`\`

**Parse Error:**
\`\`\`
$(cat "$results_dir/parse_${query_num}.txt")
\`\`\`

EOF
    fi
}

# Process each SQL file
query_counter=1
for sql_file in /Users/gregburd/src/ra/tests/book-queries/*.sql; do
    if [ -f "$sql_file" ]; then
        echo "Processing $sql_file..."
        filename=$(basename "$sql_file")

        # Extract queries (simple approach: split on semicolon at end of line)
        # Skip comment lines
        while IFS= read -r line; do
            # Skip empty lines and comment-only lines
            if [[ -z "$line" || "$line" =~ ^[[:space:]]*-- ]]; then
                continue
            fi

            # Accumulate query lines
            if [ -z "${current_query:-}" ]; then
                current_query="$line"
            else
                current_query="$current_query
$line"
            fi

            # If line ends with semicolon, test the query
            if [[ "$line" =~ \;[[:space:]]*$ ]]; then
                # Remove trailing semicolon and whitespace
                current_query="${current_query%;}"
                test_query "$filename" "$current_query" "$query_counter"
                query_counter=$((query_counter + 1))
                current_query=""
            fi
        done < "$sql_file"
    fi
done

# Write summary
cat >> "$results_file" <<EOF

- **Total Queries**: $total_queries
- **Parse Success**: $parse_success
- **Parse Failure**: $parse_failure
- **Parse Success Rate**: $(awk "BEGIN {printf \"%.2f\", 100.0 * $parse_success / $total_queries}")%
- **Optimize Success**: $optimize_success
- **Optimize Failure**: $optimize_failure
- **Optimize Success Rate**: $(awk "BEGIN {printf \"%.2f\", 100.0 * $optimize_success / $total_queries}")%

## Details

See FAILURES.md for detailed error messages for failed queries.
EOF

echo ""
echo "========================================"
echo "Test Summary"
echo "========================================"
echo "Total Queries: $total_queries"
echo -e "Parse Success: ${GREEN}$parse_success${NC}"
echo -e "Parse Failure: ${RED}$parse_failure${NC}"
echo -e "Optimize Success: ${GREEN}$optimize_success${NC}"
echo -e "Optimize Failure: ${RED}$optimize_failure${NC}"
echo ""
echo "Results written to: $results_file"
echo "Failures written to: $failures_file"
