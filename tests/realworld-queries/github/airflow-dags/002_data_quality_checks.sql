-- Data Quality Validation Queries
-- Source: Data pipelines (Great Expectations, dbt tests)
-- Pattern: OLAP - Data quality monitoring

-- Check 1: Null value detection in critical columns
SELECT
    'null_check' AS check_type,
    'orders' AS table_name,
    'user_id' AS column_name,
    COUNT(*) AS null_count,
    (COUNT(*)::DECIMAL / (SELECT COUNT(*) FROM orders)) * 100 AS null_percentage
FROM orders
WHERE user_id IS NULL
    AND created_at >= '{{ ds }}'

UNION ALL

SELECT
    'null_check' AS check_type,
    'orders' AS table_name,
    'total_amount' AS column_name,
    COUNT(*) AS null_count,
    (COUNT(*)::DECIMAL / (SELECT COUNT(*) FROM orders)) * 100 AS null_percentage
FROM orders
WHERE total_amount IS NULL
    AND created_at >= '{{ ds }}';

-- Check 2: Referential integrity violations
SELECT
    'referential_integrity' AS check_type,
    'orders' AS table_name,
    'user_id references users' AS constraint_desc,
    COUNT(*) AS violation_count
FROM orders o
LEFT JOIN users u ON o.user_id = u.id
WHERE u.id IS NULL
    AND o.created_at >= '{{ ds }}';

-- Check 3: Value range validation
WITH value_ranges AS (
    SELECT
        'range_check' AS check_type,
        'orders' AS table_name,
        'total_amount' AS column_name,
        COUNT(*) FILTER (WHERE total_amount < 0) AS negative_count,
        COUNT(*) FILTER (WHERE total_amount = 0) AS zero_count,
        COUNT(*) FILTER (WHERE total_amount > 100000) AS outlier_count,
        MIN(total_amount) AS min_value,
        MAX(total_amount) AS max_value,
        AVG(total_amount) AS avg_value,
        PERCENTILE_CONT(0.99) WITHIN GROUP (ORDER BY total_amount) AS p99_value
    FROM orders
    WHERE created_at >= '{{ ds }}'
)
SELECT
    check_type,
    table_name,
    column_name,
    negative_count,
    zero_count,
    outlier_count,
    min_value,
    max_value,
    avg_value,
    p99_value,
    CASE
        WHEN negative_count > 0 THEN 'FAIL: negative values found'
        WHEN outlier_count > (SELECT COUNT(*) FROM orders WHERE created_at >= '{{ ds }}') * 0.01
            THEN 'WARN: high outlier count'
        ELSE 'PASS'
    END AS validation_result
FROM value_ranges;

-- Check 4: Duplicate detection
WITH duplicate_check AS (
    SELECT
        user_id,
        DATE(created_at) AS order_date,
        COUNT(*) AS order_count
    FROM orders
    WHERE created_at >= '{{ ds }}'
    GROUP BY user_id, DATE(created_at)
    HAVING COUNT(*) > 10  -- Suspiciously high order count
)
SELECT
    'duplicate_check' AS check_type,
    'orders' AS table_name,
    COUNT(*) AS suspicious_user_days,
    SUM(order_count) AS total_suspicious_orders
FROM duplicate_check;

-- Check 5: Freshness validation
WITH freshness_check AS (
    SELECT
        'freshness' AS check_type,
        'orders' AS table_name,
        MAX(created_at) AS latest_timestamp,
        EXTRACT(EPOCH FROM (CURRENT_TIMESTAMP - MAX(created_at))) / 3600 AS hours_since_last_record
    FROM orders
)
SELECT
    check_type,
    table_name,
    latest_timestamp,
    hours_since_last_record,
    CASE
        WHEN hours_since_last_record > 24 THEN 'FAIL: data stale'
        WHEN hours_since_last_record > 6 THEN 'WARN: data aging'
        ELSE 'PASS'
    END AS validation_result
FROM freshness_check;

-- Check 6: Distribution consistency (compare to historical baseline)
WITH current_distribution AS (
    SELECT
        status,
        COUNT(*) AS current_count,
        (COUNT(*)::DECIMAL / SUM(COUNT(*)) OVER ()) * 100 AS current_pct
    FROM orders
    WHERE created_at >= '{{ ds }}'
    GROUP BY status
),
historical_distribution AS (
    SELECT
        status,
        AVG(daily_count) AS avg_count,
        STDDEV(daily_count) AS stddev_count
    FROM (
        SELECT
            status,
            DATE(created_at) AS order_date,
            COUNT(*) AS daily_count
        FROM orders
        WHERE created_at >= '{{ ds }}' - INTERVAL '30 days'
            AND created_at < '{{ ds }}'
        GROUP BY status, DATE(created_at)
    ) daily_stats
    GROUP BY status
)
SELECT
    cd.status,
    cd.current_count,
    cd.current_pct,
    hd.avg_count AS historical_avg,
    hd.stddev_count AS historical_stddev,
    CASE
        WHEN hd.stddev_count > 0 THEN
            ABS(cd.current_count - hd.avg_count) / hd.stddev_count
        ELSE 0
    END AS z_score,
    CASE
        WHEN hd.stddev_count > 0
            AND ABS(cd.current_count - hd.avg_count) / hd.stddev_count > 3
            THEN 'FAIL: significant deviation'
        WHEN hd.stddev_count > 0
            AND ABS(cd.current_count - hd.avg_count) / hd.stddev_count > 2
            THEN 'WARN: unusual distribution'
        ELSE 'PASS'
    END AS validation_result
FROM current_distribution cd
LEFT JOIN historical_distribution hd ON cd.status = hd.status;
