#!/usr/bin/env bash
set -euo pipefail

DB_NAME="${1:-imdb}"

echo "Validating IMDB data integrity in database '$DB_NAME'..."
echo ""

# Expected row counts from JOB paper (May 2013 IMDB snapshot)
declare -A expected=(
    ["aka_name"]=901343
    ["aka_title"]=361472
    ["cast_info"]=36244344
    ["char_name"]=3140339
    ["comp_cast_type"]=4
    ["company_name"]=234997
    ["company_type"]=4
    ["complete_cast"]=135086
    ["info_type"]=113
    ["keyword"]=134170
    ["kind_type"]=7
    ["link_type"]=18
    ["movie_companies"]=2609129
    ["movie_info"]=14835720
    ["movie_info_idx"]=1380035
    ["movie_keyword"]=4523930
    ["movie_link"]=29997
    ["name"]=4167491
    ["person_info"]=2963664
    ["role_type"]=12
    ["title"]=2528312
)

mismatches=0
total=0
total_rows=0

for table in "${!expected[@]}"; do
    expected_count=${expected[$table]}
    actual=$(psql -d "$DB_NAME" -t -A -c "SELECT COUNT(*) FROM $table" 2>/dev/null || echo "0")
    total=$((total + 1))
    total_rows=$((total_rows + actual))

    if [ "$actual" -eq "$expected_count" ]; then
        printf "✓ %-20s %12s rows (expected %s)\n" "$table:" "$actual" "$expected_count"
    else
        printf "✗ %-20s %12s rows (expected %s) - MISMATCH\n" "$table:" "$actual" "$expected_count"
        mismatches=$((mismatches + 1))
    fi
done

echo ""
echo "═══════════════════════════════════════════════════"
echo "Tables validated: $total"
echo "Total rows: $total_rows"
echo "Mismatches: $mismatches"

if [ $mismatches -eq 0 ]; then
    echo "✓ All tables have correct row counts"
    exit 0
else
    echo "✗ $mismatches table(s) have incorrect row counts"
    echo ""
    echo "Note: Some row count differences are expected if using a different"
    echo "IMDB snapshot or if the dataset was modified. The benchmark can still"
    echo "be run, but results may differ from published JOB papers."
    exit 1
fi
