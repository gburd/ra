-- Anomaly detection
-- Shows statistical analysis and outlier detection

-- Find unusual transactions based on statistical analysis
WITH account_statistics AS (
    SELECT
        debit_account_code as account_code,
        AVG(debit_amount) as mean_amount,
        STDDEV(debit_amount) as stddev_amount,
        PERCENTILE_CONT(0.25) WITHIN GROUP (ORDER BY debit_amount) as q1,
        PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY debit_amount) as median,
        PERCENTILE_CONT(0.75) WITHIN GROUP (ORDER BY debit_amount) as q3,
        COUNT(*) as transaction_count
    FROM ledger_transactions
    WHERE transaction_date >= CURRENT_DATE - INTERVAL '180 days'
    GROUP BY debit_account_code
    HAVING COUNT(*) >= 10  -- Need enough data for statistics
),
transactions_with_zscore AS (
    SELECT
        t.id,
        t.transaction_date,
        t.debit_account_code,
        t.debit_amount,
        t.description,
        s.mean_amount,
        s.stddev_amount,
        s.median,
        -- Calculate Z-score
        (t.debit_amount - s.mean_amount) / NULLIF(s.stddev_amount, 0) as z_score,
        -- Calculate IQR multiplier
        (t.debit_amount - s.median) / NULLIF(s.q3 - s.q1, 0) as iqr_score,
        -- Comparison to median
        t.debit_amount / NULLIF(s.median, 0) as median_ratio
    FROM ledger_transactions t
    JOIN account_statistics s ON t.debit_account_code = s.account_code
    WHERE t.transaction_date >= CURRENT_DATE - INTERVAL '30 days'
),
anomalies AS (
    SELECT
        *,
        CASE
            WHEN ABS(z_score) > 3 THEN 'Statistical Outlier (3σ)'
            WHEN ABS(z_score) > 2 THEN 'Unusual (2σ)'
            WHEN ABS(iqr_score) > 3 THEN 'IQR Outlier'
            WHEN median_ratio > 10 OR median_ratio < 0.1 THEN 'Extreme Ratio'
            ELSE 'Normal'
        END as anomaly_type,
        CASE
            WHEN ABS(z_score) > 3 THEN 3
            WHEN ABS(z_score) > 2 THEN 2
            WHEN ABS(iqr_score) > 3 THEN 2
            WHEN median_ratio > 10 OR median_ratio < 0.1 THEN 1
            ELSE 0
        END as severity_score
    FROM transactions_with_zscore
)
SELECT
    id,
    transaction_date,
    debit_account_code,
    debit_amount,
    ROUND(mean_amount, 2) as typical_amount,
    ROUND(median, 2) as median_amount,
    ROUND(z_score, 2) as z_score,
    ROUND(median_ratio, 2) as median_ratio,
    anomaly_type,
    severity_score,
    description
FROM anomalies
WHERE anomaly_type != 'Normal'
ORDER BY severity_score DESC, ABS(z_score) DESC
LIMIT 50;

-- Optimization opportunities:
-- 1. Materialized view for account statistics
-- 2. Partial index on recent transactions
-- 3. Parallel aggregation for statistics calculation
-- 4. Consider time-series specific optimizations