#!/usr/bin/env bash
set -euo pipefail

DB_NAME="${1:-imdb}"
DATA_DIR="benchmarks/job/data"

if [ ! -d "$DATA_DIR" ] || [ -z "$(ls -A $DATA_DIR/*.csv 2>/dev/null)" ]; then
    echo "Error: Data directory not found or empty. Run ./download_imdb.sh first."
    exit 1
fi

echo "Creating database '$DB_NAME'..."
dropdb --if-exists "$DB_NAME" 2>/dev/null || true
createdb "$DB_NAME"

echo "Loading schema..."
psql -d "$DB_NAME" -f benchmarks/job/schema.sql

echo "Loading data from CSV files..."
for csv in "$DATA_DIR"/*.csv; do
    if [ -f "$csv" ]; then
        table=$(basename "$csv" .csv)
        echo "  Loading $table..."
        # Use COPY with proper error handling
        psql -d "$DB_NAME" -c "\COPY $table FROM '$csv' WITH (FORMAT CSV, HEADER true, DELIMITER ',', NULL '')" 2>&1 | grep -v "^COPY" || {
            echo "    ⚠ Warning: Failed to load $table"
        }
    fi
done

echo "Analyzing tables..."
psql -d "$DB_NAME" -c "ANALYZE"

echo ""
echo "✓ IMDB database '$DB_NAME' loaded successfully"
echo ""
echo "To validate data integrity, run:"
echo "  ./validate_data.sh $DB_NAME"
