#!/usr/bin/env bash
set -euo pipefail

# Loads IMDB CSV data into a PostgreSQL database.
# Requires: psql, createdb (PostgreSQL client tools)
# Usage: ./load_data.sh [database_name]

DB_NAME="${1:-imdb}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DATA_DIR="${SCRIPT_DIR}/data"
SCHEMA_FILE="${SCRIPT_DIR}/schema.sql"

TABLES=(
    aka_name aka_title cast_info char_name comp_cast_type
    company_name company_type complete_cast info_type keyword
    kind_type link_type movie_companies movie_info movie_info_idx
    movie_keyword movie_link name person_info role_type title
)

if [ ! -f "$SCHEMA_FILE" ]; then
    echo "Error: Schema file not found: $SCHEMA_FILE" >&2
    exit 1
fi

if [ ! -d "$DATA_DIR" ]; then
    echo "Error: Data directory not found: $DATA_DIR" >&2
    echo "  Run ./download_imdb.sh first." >&2
    exit 1
fi

missing=0
for table in "${TABLES[@]}"; do
    if [ ! -f "$DATA_DIR/${table}.csv" ]; then
        echo "Error: Missing CSV: $DATA_DIR/${table}.csv" >&2
        missing=$((missing + 1))
    fi
done
if [ "$missing" -gt 0 ]; then
    echo "$missing CSV file(s) missing. Run ./download_imdb.sh first." >&2
    exit 1
fi

echo "Creating database '$DB_NAME'..."
dropdb --if-exists "$DB_NAME" 2>/dev/null || true
createdb "$DB_NAME"

echo "Loading schema..."
psql -d "$DB_NAME" -f "$SCHEMA_FILE" -q

echo "Loading data from CSV files..."
loaded=0
for table in "${TABLES[@]}"; do
    csv="$DATA_DIR/${table}.csv"
    printf "  %-25s" "$table..."
    # JOB CSV files: no header, comma-delimited, backslash-escaped quotes
    psql -d "$DB_NAME" -c "\\COPY $table FROM '$csv' WITH (FORMAT csv, DELIMITER ',', NULL '', ESCAPE E'\\\\');" -q
    rows=$(psql -d "$DB_NAME" -t -A -c "SELECT COUNT(*) FROM $table")
    echo "$rows rows"
    loaded=$((loaded + 1))
done

echo ""
echo "Running ANALYZE on all tables..."
psql -d "$DB_NAME" -c "ANALYZE" -q

echo ""
echo "Loaded $loaded tables into database '$DB_NAME'."
echo ""
echo "Next steps:"
echo "  ./validate_data.sh $DB_NAME    # verify row counts"
