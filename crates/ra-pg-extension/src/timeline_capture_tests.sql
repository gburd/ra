-- Timeline capture integration tests for PostgreSQL extension
--
-- These tests verify that the snapshot capture functionality works correctly
-- with PostgreSQL catalogs. Run with: psql -f timeline_capture_tests.sql

-- Load the extension
CREATE EXTENSION IF NOT EXISTS pg_ra_planner;

-- Create test schema and tables
CREATE SCHEMA IF NOT EXISTS test_timeline;

CREATE TABLE test_timeline.orders (
    order_id SERIAL PRIMARY KEY,
    customer_id INTEGER NOT NULL,
    order_date TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    total_amount NUMERIC(10, 2),
    status VARCHAR(20)
);

CREATE TABLE test_timeline.customers (
    customer_id SERIAL PRIMARY KEY,
    customer_name VARCHAR(100) NOT NULL,
    email VARCHAR(255) UNIQUE,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE test_timeline.order_items (
    item_id SERIAL PRIMARY KEY,
    order_id INTEGER REFERENCES test_timeline.orders(order_id),
    product_name VARCHAR(255) NOT NULL,
    quantity INTEGER NOT NULL,
    unit_price NUMERIC(10, 2) NOT NULL
);

-- Create indexes
CREATE INDEX idx_orders_customer ON test_timeline.orders(customer_id);
CREATE INDEX idx_orders_date ON test_timeline.orders(order_date);
CREATE INDEX idx_order_items_order ON test_timeline.order_items(order_id);

-- Insert test data
INSERT INTO test_timeline.customers (customer_name, email)
SELECT
    'Customer ' || generate_series,
    'customer' || generate_series || '@example.com'
FROM generate_series(1, 1000);

INSERT INTO test_timeline.orders (customer_id, order_date, total_amount, status)
SELECT
    (random() * 999 + 1)::INTEGER,
    CURRENT_TIMESTAMP - (random() * INTERVAL '365 days'),
    (random() * 1000)::NUMERIC(10, 2),
    CASE (random() * 4)::INTEGER
        WHEN 0 THEN 'pending'
        WHEN 1 THEN 'processing'
        WHEN 2 THEN 'shipped'
        ELSE 'delivered'
    END
FROM generate_series(1, 10000);

INSERT INTO test_timeline.order_items (order_id, product_name, quantity, unit_price)
SELECT
    (random() * 9999 + 1)::INTEGER,
    'Product ' || (random() * 100)::INTEGER,
    (random() * 10 + 1)::INTEGER,
    (random() * 100)::NUMERIC(10, 2)
FROM generate_series(1, 50000);

-- Analyze tables to generate statistics
ANALYZE test_timeline.orders;
ANALYZE test_timeline.customers;
ANALYZE test_timeline.order_items;

-- Test 1: Capture hardware profile
SELECT * FROM ra.hardware_profile();

-- Test 2: Capture snapshot as JSON
SELECT ra.capture_snapshot(ARRAY[
    'test_timeline.orders',
    'test_timeline.customers',
    'test_timeline.order_items'
]);

-- Test 3: Capture snapshot to file
SELECT ra.capture_snapshot_to_file(
    ARRAY[
        'test_timeline.orders',
        'test_timeline.customers',
        'test_timeline.order_items'
    ],
    '/tmp/timeline_snapshot_initial.toml',
    'Initial test snapshot'
);

-- Wait and modify data for second snapshot
SELECT pg_sleep(2);

-- Add more orders
INSERT INTO test_timeline.orders (customer_id, order_date, total_amount, status)
SELECT
    (random() * 999 + 1)::INTEGER,
    CURRENT_TIMESTAMP,
    (random() * 1000)::NUMERIC(10, 2),
    'pending'
FROM generate_series(1, 5000);

-- Re-analyze
ANALYZE test_timeline.orders;

-- Test 4: Capture second snapshot
SELECT ra.capture_snapshot_to_file(
    ARRAY[
        'test_timeline.orders',
        'test_timeline.customers',
        'test_timeline.order_items'
    ],
    '/tmp/timeline_snapshot_after_insert.toml',
    'After 5000 new orders'
);

-- Clean up
DROP SCHEMA test_timeline CASCADE;

-- Expected output:
-- 1. Hardware profile table showing CPU cores, memory, etc.
-- 2. JSON snapshot with schema, statistics, and facts
-- 3. Files written to /tmp/timeline_snapshot_*.toml
-- 4. Two snapshots with different statistics (row counts)

-- Verify output files
\! ls -lh /tmp/timeline_snapshot_*.toml
\! echo "=== Initial snapshot ===" && head -50 /tmp/timeline_snapshot_initial.toml
\! echo "=== After insert snapshot ===" && head -50 /tmp/timeline_snapshot_after_insert.toml
