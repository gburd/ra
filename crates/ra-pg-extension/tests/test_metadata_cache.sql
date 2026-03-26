-- Integration tests for metadata cache and relcache invalidation tracking
-- Tests that Ra automatically refreshes cached metadata when schema changes

\set ECHO all

-- Test 1: Basic cache functionality
CREATE TABLE test_cache_users (
    id INT PRIMARY KEY,
    name TEXT NOT NULL,
    email TEXT
);

-- Insert test data
INSERT INTO test_cache_users (id, name, email)
SELECT i, 'User ' || i, 'user' || i || '@example.com'
FROM generate_series(1, 1000) i;

-- Analyze to populate statistics
ANALYZE test_cache_users;

-- Query to populate cache
SELECT COUNT(*) FROM test_cache_users WHERE id < 100;

-- Check cache stats (should have 1 entry after first query)
SELECT entries > 0 AS has_cached_entries,
       hits >= 0 AS has_hits,
       misses > 0 AS has_misses
FROM ra.metadata_cache_stats();

-- Test 2: Relcache invalidation on ALTER TABLE
ALTER TABLE test_cache_users ADD COLUMN created_at TIMESTAMP DEFAULT NOW();

-- Next query should trigger metadata refresh
SELECT COUNT(*) FROM test_cache_users WHERE id < 100;

-- Check invalidations counter increased
SELECT invalidations > 0 AS detected_invalidation
FROM ra.metadata_cache_stats();

-- Test 3: Index creation triggers invalidation
CREATE INDEX idx_test_cache_users_name ON test_cache_users(name);

-- Query should see new index
SELECT COUNT(*) FROM test_cache_users WHERE name LIKE 'User 1%';

-- Check invalidations counter
SELECT invalidations >= 2 AS multiple_invalidations
FROM ra.metadata_cache_stats();

-- Test 4: Index drop triggers invalidation
DROP INDEX idx_test_cache_users_name;

-- Query should not recommend dropped index
SELECT COUNT(*) FROM test_cache_users WHERE name LIKE 'User 2%';

-- Test 5: ANALYZE triggers invalidation
UPDATE test_cache_users SET email = 'updated@example.com' WHERE id < 500;
ANALYZE test_cache_users;

-- Query uses fresh statistics
SELECT COUNT(*) FROM test_cache_users WHERE email = 'updated@example.com';

-- Check final cache stats
SELECT entries,
       invalidated,
       hits,
       misses,
       invalidations,
       ROUND(hit_rate::numeric, 2) AS hit_rate
FROM ra.metadata_cache_stats();

-- Test 6: Manual cache clear
SELECT ra.clear_metadata_cache();

-- Cache should be empty
SELECT entries = 0 AS cache_cleared
FROM ra.metadata_cache_stats();

-- Test 7: Cache repopulates on next query
SELECT COUNT(*) FROM test_cache_users;

-- Cache should have entries again
SELECT entries > 0 AS cache_repopulated
FROM ra.metadata_cache_stats();

-- Test 8: Multiple tables cached simultaneously
CREATE TABLE test_cache_orders (
    id SERIAL PRIMARY KEY,
    user_id INT REFERENCES test_cache_users(id),
    amount DECIMAL(10,2),
    created_at TIMESTAMP DEFAULT NOW()
);

INSERT INTO test_cache_orders (user_id, amount)
SELECT (random() * 999 + 1)::INT, random() * 1000
FROM generate_series(1, 5000);

ANALYZE test_cache_orders;

-- Query both tables
SELECT COUNT(*)
FROM test_cache_users u
JOIN test_cache_orders o ON u.id = o.user_id
WHERE u.id < 100;

-- Should have cached metadata for both tables
SELECT entries >= 2 AS multiple_tables_cached
FROM ra.metadata_cache_stats();

-- Test 9: Dropped table handling
CREATE TABLE test_cache_temp (id INT);
ANALYZE test_cache_temp;

-- Populate cache
SELECT COUNT(*) FROM test_cache_temp;

-- Drop table
DROP TABLE test_cache_temp;

-- Cache entry for dropped table is stale but harmless
-- Next query on other tables works fine
SELECT COUNT(*) FROM test_cache_users;

-- Test 10: Partition table invalidation
CREATE TABLE test_cache_partitioned (
    id INT,
    created_date DATE,
    data TEXT
) PARTITION BY RANGE (created_date);

CREATE TABLE test_cache_partitioned_2024 PARTITION OF test_cache_partitioned
    FOR VALUES FROM ('2024-01-01') TO ('2025-01-01');

CREATE TABLE test_cache_partitioned_2025 PARTITION OF test_cache_partitioned
    FOR VALUES FROM ('2025-01-01') TO ('2026-01-01');

INSERT INTO test_cache_partitioned (id, created_date, data)
SELECT i, '2024-01-01'::DATE + (i % 365), 'data-' || i
FROM generate_series(1, 1000) i;

ANALYZE test_cache_partitioned;

-- Query partitioned table
SELECT COUNT(*) FROM test_cache_partitioned WHERE created_date >= '2024-06-01';

-- Add new partition (triggers invalidation)
CREATE TABLE test_cache_partitioned_2026 PARTITION OF test_cache_partitioned
    FOR VALUES FROM ('2026-01-01') TO ('2027-01-01');

-- Query should detect new partition
SELECT COUNT(*) FROM test_cache_partitioned WHERE created_date >= '2024-06-01';

-- Final cache statistics
SELECT entries,
       invalidated,
       hits,
       misses,
       invalidations,
       ROUND(hit_rate::numeric, 2) AS hit_rate
FROM ra.metadata_cache_stats();

-- Cleanup
DROP TABLE test_cache_partitioned;
DROP TABLE test_cache_orders;
DROP TABLE test_cache_users;
