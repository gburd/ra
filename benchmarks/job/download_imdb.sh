#!/usr/bin/env bash
set -euo pipefail

REPO_URL="https://github.com/gregrahn/join-order-benchmark.git"
DATA_DIR="benchmarks/job/data"
QUERIES_DIR="benchmarks/job/queries"

echo "Downloading Join Order Benchmark..."
git clone --depth 1 "$REPO_URL" /tmp/job-repo

echo "Copying data files..."
mkdir -p "$DATA_DIR"
# Try multiple locations where CSV files might be
if compgen -G "/tmp/job-repo/*.csv" > /dev/null; then
    cp /tmp/job-repo/*.csv "$DATA_DIR/"
fi
if compgen -G "/tmp/job-repo/data/*.csv" > /dev/null; then
    cp /tmp/job-repo/data/*.csv "$DATA_DIR/"
fi
find /tmp/job-repo -name "*.csv" -type f -exec cp {} "$DATA_DIR/" \; 2>/dev/null || true

echo "Copying query files..."
mkdir -p "$QUERIES_DIR"
# Try multiple locations where SQL files might be
if compgen -G "/tmp/job-repo/*.sql" > /dev/null; then
    cp /tmp/job-repo/*.sql "$QUERIES_DIR/"
fi
if compgen -G "/tmp/job-repo/queries/*.sql" > /dev/null; then
    cp /tmp/job-repo/queries/*.sql "$QUERIES_DIR/"
fi
find /tmp/job-repo -name "*.sql" -type f -exec cp {} "$QUERIES_DIR/" \; 2>/dev/null || true

echo "Cleaning up..."
rm -rf /tmp/job-repo

csv_count=$(find "$DATA_DIR" -name "*.csv" -type f | wc -l | tr -d ' ')
sql_count=$(find "$QUERIES_DIR" -name "*.sql" -type f | wc -l | tr -d ' ')

echo "✓ Downloaded $csv_count CSV files"
echo "✓ Downloaded $sql_count SQL query files"

if [ "$csv_count" -eq 0 ]; then
    echo "⚠ No CSV files found. You may need to download them separately from:"
    echo "  https://github.com/gregrahn/join-order-benchmark"
fi

if [ "$sql_count" -eq 0 ]; then
    echo "⚠ No SQL files found. You may need to download them separately from:"
    echo "  https://github.com/gregrahn/join-order-benchmark"
fi
