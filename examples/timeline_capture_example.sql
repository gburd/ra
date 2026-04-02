-- Timeline Capture Example: Index Addition Scenario
--
-- This example demonstrates capturing snapshots before and after adding
-- an index to show how the query plan changes over time.
--
-- Usage: psql -d your_database -f timeline_capture_example.sql

\timing on
\echo '=== Timeline Capture Example: Index Addition Scenario ==='
\echo ''

-- Step 1: Load the RA planner extension
\echo '1. Loading pg_ra_planner extension...'
CREATE EXTENSION IF NOT EXISTS pg_ra_planner;

-- Step 2: Check hardware profile
\echo ''
\echo '2. Detected hardware profile:'
SELECT * FROM ra.hardware_profile();

-- Step 3: Create test schema
\echo ''
\echo '3. Creating test schema...'
CREATE SCHEMA IF NOT EXISTS example;

CREATE TABLE IF NOT EXISTS example.orders (
    order_id SERIAL PRIMARY KEY,
    customer_id INTEGER NOT NULL,
    order_date TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    total_amount NUMERIC(10, 2),
    status VARCHAR(20)
);

CREATE TABLE IF NOT EXISTS example.customers (
    customer_id SERIAL PRIMARY KEY,
    customer_name VARCHAR(100) NOT NULL,
    email VARCHAR(255) UNIQUE,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Step 4: Insert sample data
\echo ''
\echo '4. Inserting sample data (100K orders, 10K customers)...'
TRUNCATE example.orders, example.customers CASCADE;

INSERT INTO example.customers (customer_name, email)
SELECT
    'Customer ' || generate_series,
    'customer' || generate_series || '@example.com'
FROM generate_series(1, 10000);

INSERT INTO example.orders (customer_id, order_date, total_amount, status)
SELECT
    (random() * 9999 + 1)::INTEGER,
    CURRENT_TIMESTAMP - (random() * INTERVAL '365 days'),
    (random() * 1000)::NUMERIC(10, 2),
    CASE (random() * 4)::INTEGER
        WHEN 0 THEN 'pending'
        WHEN 1 THEN 'processing'
        WHEN 2 THEN 'shipped'
        ELSE 'delivered'
    END
FROM generate_series(1, 100000);

-- Step 5: Run ANALYZE to generate statistics
\echo ''
\echo '5. Running ANALYZE to generate statistics...'
ANALYZE example.orders;
ANALYZE example.customers;

-- Step 6: Capture snapshot BEFORE index creation
\echo ''
\echo '6. Capturing snapshot (before index)...'
SELECT ra.capture_snapshot_to_file(
    ARRAY['example.orders', 'example.customers'],
    '/tmp/snapshot_before_index.toml',
    'Before index - full table scans expected'
);

\echo 'Snapshot saved to: /tmp/snapshot_before_index.toml'

-- Step 7: Show current query plan (without index)
\echo ''
\echo '7. Query plan WITHOUT index (expect sequential scan):'
EXPLAIN (ANALYZE, BUFFERS)
SELECT customer_id, COUNT(*), SUM(total_amount)
FROM example.orders
WHERE status = 'pending'
GROUP BY customer_id
ORDER BY COUNT(*) DESC
LIMIT 10;

-- Step 8: Create index
\echo ''
\echo '8. Creating index on orders.status...'
CREATE INDEX idx_orders_status ON example.orders(status);

-- Step 9: Re-analyze to update statistics
\echo ''
\echo '9. Re-analyzing with new index...'
ANALYZE example.orders;

-- Step 10: Capture snapshot AFTER index creation
\echo ''
\echo '10. Capturing snapshot (after index)...'
SELECT ra.capture_snapshot_to_file(
    ARRAY['example.orders', 'example.customers'],
    '/tmp/snapshot_after_index.toml',
    'After index - index scans expected'
);

\echo 'Snapshot saved to: /tmp/snapshot_after_index.toml'

-- Step 11: Show new query plan (with index)
\echo ''
\echo '11. Query plan WITH index (expect bitmap index scan):'
EXPLAIN (ANALYZE, BUFFERS)
SELECT customer_id, COUNT(*), SUM(total_amount)
FROM example.orders
WHERE status = 'pending'
GROUP BY customer_id
ORDER BY COUNT(*) DESC
LIMIT 10;

-- Step 12: Summary
\echo ''
\echo '=== Capture Complete ==='
\echo ''
\echo 'Captured snapshots:'
\echo '  - /tmp/snapshot_before_index.toml'
\echo '  - /tmp/snapshot_after_index.toml'
\echo ''
\echo 'Next steps:'
\echo '  1. Merge snapshots into timeline:'
\echo '     ra-cli pg-snapshot merge-timeline \'
\echo '       --snapshot-dir /tmp \'
\echo '       --output index_timeline.toml \'
\echo '       --name "Index Addition Scenario" \'
\echo '       --description "Query plan changes with index"'
\echo ''
\echo '  2. Visualize timeline:'
\echo '     ra-cli timeline \'
\echo '       --timeline index_timeline.toml \'
\echo '       --query "SELECT customer_id, COUNT(*) FROM example.orders WHERE status = '\''pending'\'' GROUP BY customer_id" \'
\echo '       --tui'
\echo ''
\echo '  3. Export to PostgreSQL EXPLAIN format:'
\echo '     ra-cli timeline \'
\echo '       --timeline index_timeline.toml \'
\echo '       --query "..." \'
\echo '       --explain postgres'
\echo ''

-- Clean up (optional)
-- DROP SCHEMA example CASCADE;
