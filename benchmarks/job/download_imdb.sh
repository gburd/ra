#!/usr/bin/env bash
set -euo pipefail

# Downloads the IMDB dataset (CSV files) and JOB query files.
#
# CSV data: May 2013 snapshot from CWI (~1.2 GB compressed)
# Queries:  113 SQL queries from the JOB repository

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DATA_DIR="${SCRIPT_DIR}/data"
QUERIES_DIR="${SCRIPT_DIR}/queries"

DATA_URL="http://event.cwi.nl/da/job/imdb.tgz"
REPO_URL="https://github.com/gregrahn/join-order-benchmark.git"
TMP_DIR="${TMPDIR:-/tmp}/job-download-$$"

cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT

mkdir -p "$TMP_DIR"

# -- Download CSV data from CWI archive --
if [ -d "$DATA_DIR" ] && ls "$DATA_DIR"/*.csv >/dev/null 2>&1; then
    csv_count=$(find "$DATA_DIR" -maxdepth 1 -name "*.csv" | wc -l | tr -d ' ')
    echo "Data directory already has $csv_count CSV files, skipping download."
    echo "  (Delete $DATA_DIR to force re-download)"
else
    mkdir -p "$DATA_DIR"
    echo "Downloading IMDB CSV data from CWI archive..."
    echo "  URL: $DATA_URL"
    echo "  This is ~1.2 GB and may take a while."

    if command -v curl >/dev/null 2>&1; then
        curl -L --progress-bar -o "$TMP_DIR/imdb.tgz" "$DATA_URL"
    elif command -v wget >/dev/null 2>&1; then
        wget --show-progress -O "$TMP_DIR/imdb.tgz" "$DATA_URL"
    else
        echo "Error: curl or wget required" >&2
        exit 1
    fi

    echo "Extracting CSV files..."
    tar -xzf "$TMP_DIR/imdb.tgz" -C "$DATA_DIR"

    csv_count=$(find "$DATA_DIR" -maxdepth 1 -name "*.csv" | wc -l | tr -d ' ')
    echo "Extracted $csv_count CSV files to $DATA_DIR"
fi

# -- Download JOB queries from GitHub --
if [ -d "$QUERIES_DIR" ] && ls "$QUERIES_DIR"/*.sql >/dev/null 2>&1; then
    sql_count=$(find "$QUERIES_DIR" -maxdepth 1 -name "*.sql" | wc -l | tr -d ' ')
    echo "Queries directory already has $sql_count SQL files, skipping download."
    echo "  (Delete $QUERIES_DIR to force re-download)"
else
    mkdir -p "$QUERIES_DIR"
    echo "Cloning JOB query repository..."

    git clone --depth 1 "$REPO_URL" "$TMP_DIR/job-repo"

    # Copy only numbered query files (e.g., 1a.sql through 33c.sql)
    query_count=0
    for sql_file in "$TMP_DIR/job-repo"/[0-9]*.sql; do
        if [ -f "$sql_file" ]; then
            cp "$sql_file" "$QUERIES_DIR/"
            query_count=$((query_count + 1))
        fi
    done

    echo "Copied $query_count query files to $QUERIES_DIR"
fi

echo ""
echo "=== Download Summary ==="
csv_count=$(find "$DATA_DIR" -maxdepth 1 -name "*.csv" 2>/dev/null | wc -l | tr -d ' ')
sql_count=$(find "$QUERIES_DIR" -maxdepth 1 -name "*.sql" 2>/dev/null | wc -l | tr -d ' ')
echo "  CSV data files: $csv_count (expected: 21)"
echo "  SQL query files: $sql_count (expected: 113)"

if [ "$csv_count" -eq 0 ]; then
    echo ""
    echo "WARNING: No CSV files found. The CWI archive may be unavailable."
    echo "  Alternative: export from a PostgreSQL IMDB database."
    echo "  See: https://github.com/gregrahn/join-order-benchmark#readme"
fi
