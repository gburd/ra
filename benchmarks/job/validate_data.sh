#!/usr/bin/env bash
set -euo pipefail

# Validates IMDB row counts against expected values from the JOB paper.
# Requires: psql (PostgreSQL client)
# Usage: ./validate_data.sh [database_name]

DB_NAME="${1:-imdb}"

echo "Validating IMDB data in database '$DB_NAME'..."
echo ""

# Expected row counts from the JOB paper (May 2013 IMDB snapshot).
# Sorted alphabetically for deterministic output.
TABLES=(
    aka_name aka_title cast_info char_name comp_cast_type
    company_name company_type complete_cast info_type keyword
    kind_type link_type movie_companies movie_info movie_info_idx
    movie_keyword movie_link name person_info role_type title
)
EXPECTED=(
    901343 361472 36244344 3140339 4
    234997 4 135086 113 134170
    7 18 2609129 14835720 1380035
    4523930 29997 4167491 2963664 12 2528312
)

mismatches=0
total=0
total_rows=0

for i in "${!TABLES[@]}"; do
    table="${TABLES[$i]}"
    expected_count="${EXPECTED[$i]}"
    actual=$(psql -d "$DB_NAME" -t -A -c \
        "SELECT COUNT(*) FROM $table" 2>/dev/null || echo "0")
    total=$((total + 1))
    total_rows=$((total_rows + actual))

    if [ "$actual" -eq "$expected_count" ]; then
        printf "  OK  %-25s %12s rows\n" "$table" "$actual"
    else
        printf "  ERR %-25s %12s rows (expected %s)\n" \
            "$table" "$actual" "$expected_count"
        mismatches=$((mismatches + 1))
    fi
done

echo ""
echo "================================================="
echo "Tables validated: $total"
echo "Total rows: $total_rows"
echo "Mismatches: $mismatches"

if [ "$mismatches" -eq 0 ]; then
    echo "All tables have correct row counts."
    exit 0
else
    echo "$mismatches table(s) have incorrect row counts."
    echo ""
    echo "Note: Row count differences are expected if using a different"
    echo "IMDB snapshot. The benchmark can still run, but results may"
    echo "differ from published JOB papers."
    exit 1
fi
