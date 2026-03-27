#!/usr/bin/env bash
# Test DocumentDB-style query with proper shell escaping

# Method 1: Use $'...' syntax which allows \' for escaped quotes
echo "=== Method 1: Using \$'...' syntax ==="
cargo run --quiet --bin ra-cli -- optimize --resource-budget unlimited --rules --verbose \
  $'SELECT document FROM documentdb_api.collection(\'mydb\', \'users\')\nWHERE document @> \'{"age": {"$gt": 25}}\''

echo ""
echo "=== Method 2: Double quotes with escaped single quotes ==="
cargo run --quiet --bin ra-cli -- optimize --resource-budget unlimited --rules --verbose \
  "SELECT document FROM documentdb_api.collection('mydb', 'users')
WHERE document @> '{\"age\": {\"\$gt\": 25}}'"

echo ""
echo "=== Method 3: Using @? instead of @= (standard PostgreSQL) ==="
cargo run --quiet --bin ra-cli -- optimize --resource-budget unlimited --rules --verbose \
  "SELECT document FROM documentdb_api.collection('mydb', 'users')
WHERE document @> '{\"age\": {\"\$gt\": 25}}'
  AND document @? '\$.status ? (@ == \"active\")'"

echo ""
echo "Note: @= is not a standard PostgreSQL operator."
echo "Use @> (contains) or @? (path exists) instead."
