-- dbt Model: Daily Metrics Rollup
-- Source: SaaS analytics pipelines
-- Pattern: OLAP - Time-series aggregation with window functions

WITH daily_order_metrics AS (
    SELECT
        DATE_TRUNC('day', created_at) AS metric_date,
        COUNT(*) AS order_count,
        COUNT(DISTINCT user_id) AS unique_customers,
        SUM(total_amount) AS total_revenue,
        AVG(total_amount) AS avg_order_value,
        PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY total_amount) AS median_order_value,
        PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY total_amount) AS p95_order_value
    FROM orders
    WHERE status = 'completed'
        AND created_at >= CURRENT_DATE - INTERVAL '90 days'
    GROUP BY DATE_TRUNC('day', created_at)
),

daily_user_metrics AS (
    SELECT
        DATE_TRUNC('day', created_at) AS metric_date,
        COUNT(*) AS new_users
    FROM users
    WHERE created_at >= CURRENT_DATE - INTERVAL '90 days'
    GROUP BY DATE_TRUNC('day', created_at)
),

combined_metrics AS (
    SELECT
        COALESCE(dom.metric_date, dum.metric_date) AS metric_date,
        COALESCE(dom.order_count, 0) AS order_count,
        COALESCE(dom.unique_customers, 0) AS unique_customers,
        COALESCE(dom.total_revenue, 0) AS total_revenue,
        COALESCE(dom.avg_order_value, 0) AS avg_order_value,
        COALESCE(dom.median_order_value, 0) AS median_order_value,
        COALESCE(dom.p95_order_value, 0) AS p95_order_value,
        COALESCE(dum.new_users, 0) AS new_users
    FROM daily_order_metrics dom
    FULL OUTER JOIN daily_user_metrics dum ON dom.metric_date = dum.metric_date
)

SELECT
    metric_date,
    order_count,
    unique_customers,
    total_revenue,
    avg_order_value,
    median_order_value,
    p95_order_value,
    new_users,
    -- 7-day moving averages
    AVG(order_count) OVER (
        ORDER BY metric_date
        ROWS BETWEEN 6 PRECEDING AND CURRENT ROW
    ) AS order_count_7d_ma,
    AVG(total_revenue) OVER (
        ORDER BY metric_date
        ROWS BETWEEN 6 PRECEDING AND CURRENT ROW
    ) AS revenue_7d_ma,
    -- Week-over-week growth
    LAG(total_revenue, 7) OVER (ORDER BY metric_date) AS revenue_7d_ago,
    CASE
        WHEN LAG(total_revenue, 7) OVER (ORDER BY metric_date) > 0 THEN
            ((total_revenue - LAG(total_revenue, 7) OVER (ORDER BY metric_date))
             / LAG(total_revenue, 7) OVER (ORDER BY metric_date)) * 100
        ELSE NULL
    END AS revenue_wow_growth_pct
FROM combined_metrics
ORDER BY metric_date DESC;
