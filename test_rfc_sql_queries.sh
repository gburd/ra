#!/usr/bin/env bash
set -euo pipefail

# Test all SQL queries found in RFC documents
# This script extracts SQL code blocks from RFCs and tests them with ra-cli

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Counters
TOTAL=0
PASSED=0
FAILED=0

# Output files
REPORT_FILE="rfc_sql_test_report.md"
FAILED_FILE="rfc_sql_failed_queries.txt"

echo "# RFC SQL Query Test Report" > "$REPORT_FILE"
echo "Generated: $(date)" >> "$REPORT_FILE"
echo "" >> "$REPORT_FILE"

> "$FAILED_FILE"

# Function to test a single SQL query
test_query() {
    local query="$1"
    local source="$2"
    local line_num="$3"

    TOTAL=$((TOTAL + 1))

    # Write query to temporary file
    local tmpfile=$(mktemp)
    echo "$query" > "$tmpfile"

    # Test with ra-cli optimize
    if cargo run --bin ra-cli -- optimize < "$tmpfile" > /dev/null 2>&1; then
        PASSED=$((PASSED + 1))
        echo -e "${GREEN}✓${NC} Query #$TOTAL from $source:$line_num"
        echo "**Query #$TOTAL** from \`$source\`:$line_num - ✅ PASS" >> "$REPORT_FILE"
    else
        FAILED=$((FAILED + 1))
        echo -e "${RED}✗${NC} Query #$TOTAL from $source:$line_num"
        echo "**Query #$TOTAL** from \`$source\`:$line_num - ❌ FAIL" >> "$REPORT_FILE"
        echo "" >> "$REPORT_FILE"
        echo '```sql' >> "$REPORT_FILE"
        echo "$query" >> "$REPORT_FILE"
        echo '```' >> "$REPORT_FILE"
        echo "" >> "$REPORT_FILE"

        # Also save to failed queries file
        echo "=== Query #$TOTAL from $source:$line_num ===" >> "$FAILED_FILE"
        echo "$query" >> "$FAILED_FILE"
        echo "" >> "$FAILED_FILE"
    fi

    rm -f "$tmpfile"
}

echo "Testing SQL queries from RFC documents..."
echo ""

# RFC 0053: Stored Procedure Dialect Support
echo "Testing RFC 0053..."
test_query "CREATE OR REPLACE FUNCTION get_customer_orders(customer_id INTEGER)
RETURNS TABLE(order_id INTEGER, total NUMERIC) AS \$\$
BEGIN
    RETURN QUERY
    SELECT o.id, o.total
    FROM orders o
    WHERE o.customer_id = get_customer_orders.customer_id
      AND o.status = 'completed'
    ORDER BY o.created_at DESC;
END;
\$\$ LANGUAGE plpgsql;" "RFC 0053" "40"

test_query "SELECT id, amount FROM orders WHERE status = 'pending'" "RFC 0053" "69"

test_query "UPDATE orders SET status = 'processing' WHERE id = order_rec.id" "RFC 0053" "71"

test_query "INSERT INTO batch_summary (total_amount, processed_at)
VALUES (total, NOW())" "RFC 0053" "79"

test_query "SELECT balance INTO v_balance
FROM accounts WHERE account_id = p_from_acct
FOR UPDATE" "RFC 0053" "103"

test_query "UPDATE accounts SET balance = balance - p_amount
WHERE account_id = p_from_acct" "RFC 0053" "111"

test_query "UPDATE accounts SET balance = balance + p_amount
WHERE account_id = p_to_acct" "RFC 0053" "114"

test_query "INSERT INTO transactions (from_acct, to_acct, amount, txn_date)
VALUES (p_from_acct, p_to_acct, p_amount, SYSDATE)" "RFC 0053" "117"

test_query "SELECT product_id, SUM(quantity) AS qty
FROM inventory_movements
WHERE warehouse_id = @warehouse_id
GROUP BY product_id" "RFC 0053" "147"

test_query "UPDATE inventory_levels
SET quantity = @qty, last_updated = GETDATE()
WHERE warehouse_id = @warehouse_id
  AND product_id = @product_id" "RFC 0053" "157"

test_query "INSERT INTO error_log (message, error_number, occurred_at)
VALUES (ERROR_MESSAGE(), ERROR_NUMBER(), GETDATE())" "RFC 0053" "172"

test_query "SELECT order_id, total_amount
FROM orders WHERE campaign = campaign_id" "RFC 0053" "194"

test_query "UPDATE orders
SET discount = v_total * 0.10
WHERE order_id = v_order_id" "RFC 0053" "206"

test_query "UPDATE orders
SET discount = 0
WHERE order_id = v_order_id" "RFC 0053" "210"

# RFC 0055: RDBMS-Specific Type Support
echo "Testing RFC 0055..."
test_query "SELECT user_id, data->>'name' AS name
FROM users
WHERE data->>'status' = 'active'
  AND data->>'verified' = 'true'" "RFC 0055" "46"

# RFC 0056: PostgreSQL Type Optimizations
echo "Testing RFC 0056..."
test_query "SELECT id, data->>'name', data->>'email'
FROM users
WHERE data->>'status' = 'active'
  AND data->>'verified' = 'true'
  AND data->>'country' = 'US'" "RFC 0056" "68"

test_query "SELECT * FROM articles
WHERE status = 'published'
ORDER BY created DESC
LIMIT 10" "RFC 0056" "125"

test_query "SELECT doc_id, xpath('//author/text()', xmldoc) AS author
FROM documents
WHERE xpath_exists('//published[@year=\"2025\"]', xmldoc)" "RFC 0056" "154"

test_query "SELECT * FROM posts WHERE tags @> ARRAY['postgresql', 'optimization']" "RFC 0056" "550"

test_query "SELECT tag, COUNT(*)
FROM posts, unnest(tags) AS tag
GROUP BY tag" "RFC 0056" "561"

# RFC 0057: Cross-Database Type Adaptation
echo "Testing RFC 0057..."
test_query "SELECT id FROM orders WHERE data contains {\"status\": \"shipped\"}" "RFC 0057" "46"

# Note: Most queries in 0057 are dialect-specific examples

# RFC 0061: PostgreSQL Extension-Aware Optimization
echo "Testing RFC 0061..."
test_query "SELECT extname, extversion
FROM pg_extension
WHERE extname IN (
    'postgis', 'timescaledb', 'citus',
    'hstore', 'ltree', 'pg_trgm', 'citext',
    'bloom', 'btree_gin', 'btree_gist',
    'pg_partman', 'pg_cron', 'pg_stat_statements'
)" "RFC 0061" "86"

test_query "SELECT name, ST_AsText(geom)
FROM buildings
WHERE ST_DWithin(geom, ST_MakePoint(-73.97, 40.77)::geography, 500)" "RFC 0061" "103"

test_query "SELECT time_bucket('1 hour', time) AS bucket,
       device_id,
       avg(temperature)
FROM sensor_data
WHERE time > now() - interval '7 days'
GROUP BY bucket, device_id" "RFC 0061" "124"

test_query "SELECT o.order_id, c.name, sum(oi.quantity)
FROM orders o
JOIN customers c ON o.customer_id = c.customer_id
JOIN order_items oi ON o.order_id = oi.order_id
WHERE o.region = 'US'
GROUP BY o.order_id, c.name" "RFC 0061" "147"

test_query "SELECT a.attname,
       a.attstorage,
       s.avg_width
FROM pg_attribute a
JOIN pg_stats s
  ON s.tablename = a.attrelid::regclass::text
 AND s.attname = a.attname
WHERE a.attrelid = 'articles'::regclass
  AND a.attstorage IN ('x', 'e', 'm')
  AND a.atttypid IN (
      'text'::regtype, 'bytea'::regtype,
      'jsonb'::regtype, 'json'::regtype, 'xml'::regtype
  )
  AND s.avg_width > 2048" "RFC 0061" "358"

test_query "SELECT logicalrelid::text AS table_name,
       partmethod,
       partkey
FROM pg_dist_partition" "RFC 0061" "592"

test_query "SELECT h.table_name,
       d.column_name AS time_column,
       d.partitioning_func
FROM _timescaledb_catalog.hypertable h
JOIN _timescaledb_catalog.dimension d
  ON h.id = d.hypertable_id
WHERE d.column_type = 'timestamptz'::regtype" "RFC 0061" "655"

test_query "SELECT query,
       calls,
       mean_exec_time,
       stddev_exec_time,
       rows,
       shared_blks_hit,
       shared_blks_read
FROM pg_stat_statements
WHERE dbid = current_database()::oid
ORDER BY total_exec_time DESC
LIMIT 100" "RFC 0061" "772"

# RFC 0063: Spatial Query Optimization
echo "Testing RFC 0063..."
test_query "SELECT b.name, p.type
FROM buildings b
JOIN parcels p ON ST_Within(b.geom, p.geom)
WHERE ST_DWithin(b.geom, ST_MakePoint(-73.97, 40.77)::geometry, 1000)" "RFC 0063" "81"

# RFC 0065: Time-Series Query Optimization
echo "Testing RFC 0065..."
test_query "SELECT h.table_name, h.schema_name,
       d.column_name AS time_column,
       d.interval_length AS chunk_interval,
       (SELECT count(*) FROM _timescaledb_catalog.chunk c
        WHERE c.hypertable_id = h.id) AS chunk_count
FROM _timescaledb_catalog.hypertable h
JOIN _timescaledb_catalog.dimension d
  ON h.id = d.hypertable_id
WHERE d.num_slices IS NULL" "RFC 0065" "61"

test_query "SELECT time_bucket('1 hour', time) AS bucket,
       avg(temperature)
FROM sensor_data
WHERE time BETWEEN '2026-03-18' AND '2026-03-24'
GROUP BY bucket" "RFC 0065" "76"

test_query "SELECT time_bucket('1 hour', time) AS bucket,
       device_id,
       avg(temperature) AS avg_temp
FROM sensor_data
WHERE time > now() - interval '30 days'
GROUP BY 1, 2" "RFC 0065" "104"

test_query "SELECT bucket, device_id, avg_temp
FROM hourly_temps
WHERE bucket > now() - interval '30 days'" "RFC 0065" "115"

test_query "SELECT DISTINCT ON (device_id) *
FROM sensor_data
ORDER BY device_id, time DESC" "RFC 0065" "200"

test_query "SELECT d.name, avg(s.temperature)
FROM sensor_data s
JOIN devices d ON s.device_id = d.id
WHERE s.time > now() - interval '7 days'
GROUP BY d.name" "RFC 0065" "233"

# RFC 0067: Full-Text Search Optimization
echo "Testing RFC 0067..."
test_query "SELECT * FROM articles
WHERE document_tsv @@ to_tsquery('english', 'search & terms')" "RFC 0067" "59"

test_query "SELECT * FROM articles
WHERE to_tsvector('english', title || ' ' || body) @@ plainto_tsquery('search terms')" "RFC 0067" "62"

test_query "SELECT *, ts_rank(document_tsv, query) AS rank
FROM articles, plainto_tsquery('search') AS query
WHERE document_tsv @@ query
ORDER BY rank DESC
LIMIT 10" "RFC 0067" "65"

test_query "SELECT * FROM articles
WHERE title LIKE '%search%'" "RFC 0067" "72"

test_query "SELECT * FROM articles
WHERE document_tsv @@ to_tsquery('postgresql')
  AND category = 'database'
  AND published_date > '2026-01-01'" "RFC 0067" "224"

# RFC 0079: PostgreSQL RUM Index
echo "Testing RFC 0079..."
test_query "SELECT EXISTS(SELECT 1 FROM pg_am WHERE amname = 'rum')" "RFC 0079" "63"

test_query "SELECT *, ts_rank(body_tsv, q) AS rank
FROM articles, plainto_tsquery('postgresql optimization') AS q
WHERE body_tsv @@ q
ORDER BY rank DESC
LIMIT 10" "RFC 0079" "117"

test_query "SELECT *, body_tsv <=> plainto_tsquery('postgresql optimization') AS dist
FROM articles
WHERE body_tsv @@ plainto_tsquery('postgresql optimization')
ORDER BY dist
LIMIT 10" "RFC 0079" "124"

# RFC 0080: DocumentDB RUM BSON Optimization
echo "Testing RFC 0080..."
test_query "SELECT extname, extversion
FROM pg_extension
WHERE extname IN ('documentdb_core', 'documentdb',
                  'documentdb_extended_rum')" "RFC 0080" "70"

test_query "SELECT documentdb_api_internal.create_indexes_non_concurrently(
  'mydb',
  '{\"createIndexes\": \"articles\",
    \"indexes\": [{\"key\": {\"content\": \"text\"},
                 \"name\": \"idx_content_text\"}]}'::bson
)" "RFC 0080" "106"

# RFC 0082: MongoDB Formal Semantics
echo "Testing RFC 0082..."
test_query "SELECT jsonb_path_query(data, '\$.items[*]') FROM docs WHERE data->>'status' = 'active'" "RFC 0082" "360"

# RFC 0083: XPath/XQuery Optimization
echo "Testing RFC 0083..."
test_query "SELECT * FROM docs
WHERE (xpath('/doc/status/text()', data))[1]::text = 'active'" "RFC 0083" "102"

test_query "SELECT * FROM docs
WHERE xmlexists('/doc/status[text()=\"active\"]' PASSING data)" "RFC 0083" "106"

test_query "SELECT * FROM docs
WHERE XMLQuery('/doc/items/item[@price > 100]'
  PASSING data RETURNING CONTENT) IS NOT NULL" "RFC 0083" "113"

test_query "SELECT * FROM docs d
WHERE existsNode(d.data, '/doc/items/item[@price > 100]') = 1" "RFC 0083" "118"

test_query "SELECT * FROM docs
WHERE data.query('/doc/status') IS NOT NULL" "RFC 0083" "124"

test_query "SELECT * FROM docs
WHERE data.exist('/doc/status') = 1" "RFC 0083" "129"

# RFC 0085: Platform-Specific Rule Architecture
echo "Testing RFC 0085..."
test_query "SELECT extname, extversion
FROM pg_extension
WHERE extname IN ('citus', 'postgis', 'timescaledb', 'rum', 'pg_partman')" "RFC 0085" "174"

test_query "SELECT comp_name, status
FROM dba_registry
WHERE comp_name IN ('XML Database', 'Advanced Queuing', 'Spatial')" "RFC 0085" "181"

# Summary
echo ""
echo "================================"
echo "Test Results Summary"
echo "================================"
echo -e "Total queries tested: $TOTAL"
echo -e "${GREEN}Passed: $PASSED${NC}"
echo -e "${RED}Failed: $FAILED${NC}"
echo ""

if [ $FAILED -gt 0 ]; then
    echo -e "${YELLOW}Failed queries have been saved to: $FAILED_FILE${NC}"
fi

echo "Full report saved to: $REPORT_FILE"
echo ""

# Write summary to report
echo "" >> "$REPORT_FILE"
echo "## Summary" >> "$REPORT_FILE"
echo "" >> "$REPORT_FILE"
echo "- **Total queries tested:** $TOTAL" >> "$REPORT_FILE"
echo "- **Passed:** $PASSED" >> "$REPORT_FILE"
echo "- **Failed:** $FAILED" >> "$REPORT_FILE"
echo "- **Success rate:** $(awk "BEGIN {printf \"%.1f%%\", ($PASSED/$TOTAL)*100}")" >> "$REPORT_FILE"

# Exit with error if any tests failed
if [ $FAILED -gt 0 ]; then
    exit 1
fi
