-- ============================================================================
-- Citus Distributed Schema for Ra CLI Demo
-- ============================================================================
-- This schema creates distributed tables across the Citus cluster
-- and populates them with sample data for distributed query optimization demos
-- ============================================================================

-- Drop existing tables if they exist
DROP TABLE IF EXISTS events CASCADE;
DROP TABLE IF EXISTS users CASCADE;
DROP TABLE IF EXISTS products CASCADE;
DROP TABLE IF EXISTS user_sessions CASCADE;

-- ============================================================================
-- 1. USERS TABLE - Distributed by user_id (primary distribution key)
-- ============================================================================
CREATE TABLE users (
    user_id bigint PRIMARY KEY,
    email varchar(255) NOT NULL UNIQUE,
    first_name varchar(100),
    last_name varchar(100),
    country varchar(50),
    subscription_tier varchar(20) CHECK (subscription_tier IN ('free', 'premium', 'enterprise')),
    created_at timestamp DEFAULT CURRENT_TIMESTAMP,
    last_login timestamp
);

-- Distribute users table by user_id across worker nodes
SELECT create_distributed_table('users', 'user_id');

-- ============================================================================
-- 2. EVENTS TABLE - Distributed by user_id (co-located with users)
-- ============================================================================
CREATE TABLE events (
    event_id bigserial,
    user_id bigint NOT NULL,
    session_id varchar(100),
    event_type varchar(50) NOT NULL,
    event_time timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP,
    properties jsonb,
    created_at timestamp DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (event_id, user_id)  -- Include distribution key in PK
);

-- Distribute events table by user_id (co-located with users)
SELECT create_distributed_table('events', 'user_id');

-- ============================================================================
-- 3. PRODUCTS TABLE - Reference table (replicated on all nodes)
-- ============================================================================
CREATE TABLE products (
    product_id int PRIMARY KEY,
    name varchar(255) NOT NULL,
    category varchar(100),
    price decimal(10,2),
    description text,
    created_at timestamp DEFAULT CURRENT_TIMESTAMP
);

-- Create reference table (replicated on all worker nodes)
SELECT create_reference_table('products');

-- ============================================================================
-- 4. USER_SESSIONS TABLE - Distributed by session_id (different from user_id)
-- ============================================================================
CREATE TABLE user_sessions (
    session_id varchar(100) PRIMARY KEY,
    user_id bigint NOT NULL,
    start_time timestamp NOT NULL,
    end_time timestamp,
    duration_seconds int,
    page_views int DEFAULT 0,
    device_type varchar(50),
    browser varchar(50),
    ip_address inet
);

-- Distribute by session_id to demonstrate cross-shard joins
SELECT create_distributed_table('user_sessions', 'session_id');

-- ============================================================================
-- POPULATE REFERENCE TABLE (products) - Available on all nodes
-- ============================================================================
INSERT INTO products (product_id, name, category, price) VALUES
(1, 'Premium Analytics Dashboard', 'Software', 299.00),
(2, 'Data Visualization Pro', 'Software', 199.00),
(3, 'Query Optimizer Enterprise', 'Software', 999.00),
(4, 'Database Performance Monitor', 'Software', 149.00),
(5, 'Cloud Storage 1TB', 'Storage', 9.99),
(6, 'Cloud Storage 10TB', 'Storage', 99.99),
(7, 'AI Model Training Credits', 'Compute', 299.00),
(8, 'GPU Compute Hours', 'Compute', 1.50),
(9, 'Advanced Security Suite', 'Security', 199.00),
(10, 'Backup & Recovery Service', 'Service', 49.99),
(11, 'Real-time Analytics Engine', 'Software', 599.00),
(12, 'Machine Learning Platform', 'Software', 799.00),
(13, 'Data Pipeline Orchestrator', 'Software', 399.00),
(14, 'Business Intelligence Suite', 'Software', 1299.00),
(15, 'API Gateway Premium', 'Software', 99.00);

-- ============================================================================
-- POPULATE DISTRIBUTED TABLES WITH SAMPLE DATA
-- ============================================================================

-- Generate users across different countries and subscription tiers
INSERT INTO users (user_id, email, first_name, last_name, country, subscription_tier, created_at, last_login)
SELECT
    user_id,
    'user' || user_id || '@example.com',
    'User',
    user_id::text,
    CASE (user_id % 10)
        WHEN 0 THEN 'USA'
        WHEN 1 THEN 'Canada'
        WHEN 2 THEN 'UK'
        WHEN 3 THEN 'Germany'
        WHEN 4 THEN 'France'
        WHEN 5 THEN 'Japan'
        WHEN 6 THEN 'Australia'
        WHEN 7 THEN 'Brazil'
        WHEN 8 THEN 'India'
        ELSE 'Netherlands'
    END,
    CASE (user_id % 3)
        WHEN 0 THEN 'free'
        WHEN 1 THEN 'premium'
        ELSE 'enterprise'
    END,
    CURRENT_TIMESTAMP - (random() * interval '365 days'),
    CURRENT_TIMESTAMP - (random() * interval '30 days')
FROM generate_series(1, 5000) AS user_id;

-- Generate user sessions with various patterns
INSERT INTO user_sessions (session_id, user_id, start_time, end_time, duration_seconds, page_views, device_type, browser)
SELECT
    'sess_' || user_id || '_' || session_num,
    user_id,
    CURRENT_TIMESTAMP - (random() * interval '60 days'),
    NULL, -- Will calculate end_time
    (300 + random() * 3600)::int, -- 5 minutes to 1 hour
    (1 + random() * 50)::int,
    CASE (session_num % 4)
        WHEN 0 THEN 'desktop'
        WHEN 1 THEN 'mobile'
        WHEN 2 THEN 'tablet'
        ELSE 'mobile'
    END,
    CASE (session_num % 3)
        WHEN 0 THEN 'Chrome'
        WHEN 1 THEN 'Firefox'
        ELSE 'Safari'
    END
FROM generate_series(1, 3000) AS user_id,
     generate_series(1, 3) AS session_num
WHERE user_id <= 3000;

-- Update session end times
UPDATE user_sessions
SET end_time = start_time + (duration_seconds * interval '1 second');

-- Generate events with realistic patterns
INSERT INTO events (user_id, session_id, event_type, event_time, properties)
SELECT
    user_id,
    session_id,
    event_types.event_type,
    start_time + (random() * (COALESCE(end_time, start_time + interval '1 hour') - start_time)),
    CASE
        WHEN event_types.event_type = 'purchase' THEN
            jsonb_build_object(
                'product_id', (1 + random() * 15)::int,
                'amount', round((random() * 500 + 10)::numeric, 2),
                'payment_method', CASE (random() * 3)::int
                    WHEN 0 THEN 'credit_card'
                    WHEN 1 THEN 'paypal'
                    ELSE 'bank_transfer'
                END
            )
        WHEN event_types.event_type = 'page_view' THEN
            jsonb_build_object(
                'page', CASE (random() * 5)::int
                    WHEN 0 THEN '/dashboard'
                    WHEN 1 THEN '/analytics'
                    WHEN 2 THEN '/reports'
                    WHEN 3 THEN '/settings'
                    ELSE '/profile'
                END,
                'load_time_ms', (50 + random() * 2000)::int
            )
        WHEN event_types.event_type = 'api_call' THEN
            jsonb_build_object(
                'endpoint', '/api/v1/' || CASE (random() * 4)::int
                    WHEN 0 THEN 'users'
                    WHEN 1 THEN 'analytics'
                    WHEN 2 THEN 'reports'
                    ELSE 'data'
                END,
                'method', CASE (random() * 3)::int
                    WHEN 0 THEN 'GET'
                    WHEN 1 THEN 'POST'
                    ELSE 'PUT'
                END,
                'response_time_ms', (10 + random() * 500)::int
            )
        ELSE
            jsonb_build_object(
                'feature', CASE (random() * 4)::int
                    WHEN 0 THEN 'dashboard'
                    WHEN 1 THEN 'charts'
                    WHEN 2 THEN 'export'
                    ELSE 'share'
                END
            )
    END
FROM user_sessions us
CROSS JOIN (
    VALUES
        ('login', 1),
        ('page_view', 8),
        ('feature_usage', 5),
        ('api_call', 3),
        ('purchase', 1),
        ('logout', 1)
) AS event_types(event_type, weight),
generate_series(1, event_types.weight) AS event_instance
WHERE us.start_time >= CURRENT_TIMESTAMP - interval '90 days';

-- Add some more recent high-value events for interesting analytics
INSERT INTO events (user_id, session_id, event_type, event_time, properties)
SELECT
    user_id,
    'recent_' || user_id,
    'purchase',
    CURRENT_TIMESTAMP - (random() * interval '7 days'),
    jsonb_build_object(
        'product_id', 11 + (random() * 4)::int, -- High-value products
        'amount', round((random() * 1000 + 500)::numeric, 2),
        'payment_method', 'credit_card',
        'promotion_code', 'NEWUSER2024'
    )
FROM generate_series(1000, 1500) AS user_id
WHERE random() < 0.3; -- 30% chance

-- ============================================================================
-- CREATE INDEXES FOR BETTER QUERY PERFORMANCE
-- ============================================================================

-- Indexes on distributed tables
CREATE INDEX idx_events_time ON events (event_time);
CREATE INDEX idx_events_type ON events (event_type);
CREATE INDEX idx_events_user_time ON events (user_id, event_time);
CREATE INDEX idx_events_session ON events (session_id);

CREATE INDEX idx_users_country ON users (country);
CREATE INDEX idx_users_tier ON users (subscription_tier);
CREATE INDEX idx_users_created ON users (created_at);

CREATE INDEX idx_sessions_user ON user_sessions (user_id);
CREATE INDEX idx_sessions_start ON user_sessions (start_time);
CREATE INDEX idx_sessions_device ON user_sessions (device_type);

CREATE INDEX idx_products_category ON products (category);
CREATE INDEX idx_products_price ON products (price);

-- ============================================================================
-- VERIFY DISTRIBUTION AND GATHER STATISTICS
-- ============================================================================

-- Update statistics for the distributed tables
ANALYZE users;
ANALYZE events;
ANALYZE products;
ANALYZE user_sessions;

-- Show distribution information
SELECT 'Distribution Summary' AS info;
SELECT
    schemaname,
    tablename,
    citus_table_type(logicalrelid) AS table_type,
    distribution_column,
    shard_count
FROM citus_tables
ORDER BY tablename;

-- Show sample of data across tables
SELECT 'Sample Users' AS info;
SELECT user_id, email, country, subscription_tier
FROM users
ORDER BY user_id
LIMIT 5;

SELECT 'Sample Events' AS info;
SELECT event_id, user_id, event_type, event_time, properties
FROM events
ORDER BY event_time DESC
LIMIT 5;

SELECT 'Sample Sessions' AS info;
SELECT session_id, user_id, start_time, duration_seconds, device_type
FROM user_sessions
ORDER BY start_time DESC
LIMIT 5;

SELECT 'Table Row Counts' AS info;
SELECT 'users'::text AS table_name, COUNT(*) AS row_count FROM users
UNION ALL
SELECT 'events'::text, COUNT(*) FROM events
UNION ALL
SELECT 'products'::text, COUNT(*) FROM products
UNION ALL
SELECT 'user_sessions'::text, COUNT(*) FROM user_sessions;

-- Show worker node distribution
SELECT 'Shard Distribution Across Workers' AS info;
SELECT
    nodename,
    nodeport,
    COUNT(*) AS shard_count
FROM citus_shards cs
JOIN citus_shard_placement csp ON cs.shardid = csp.shardid
WHERE csp.shardstate = 1  -- Active shards
GROUP BY nodename, nodeport
ORDER BY nodename, nodeport;

\echo ''
\echo '============================================================================'
\echo 'Citus distributed schema setup complete!'
\echo ''
\echo 'Tables created and distributed:'
\echo '  📊 users:         Distributed by user_id (co-location group)'
\echo '  📈 events:        Distributed by user_id (co-located with users)'
\echo '  📋 products:      Reference table (replicated on all nodes)'
\echo '  🔗 user_sessions: Distributed by session_id (cross-shard joins)'
\echo ''
\echo 'Sample data loaded:'
\echo '  👥 5,000 users across 10 countries'
\echo '  🎯 ~50,000+ events with realistic patterns'
\echo '  📦 15 products in various categories'
\echo '  💻 ~9,000 user sessions'
\echo ''
\echo 'Ready for distributed query optimization demos!'
\echo '============================================================================'
\echo ''