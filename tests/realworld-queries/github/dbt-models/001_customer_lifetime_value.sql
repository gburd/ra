-- dbt Model: Customer Lifetime Value
-- Source: dbt analytics projects
-- Pattern: OLAP - Data warehouse transformation

WITH customer_orders AS (
    SELECT
        user_id,
        COUNT(DISTINCT id) AS order_count,
        SUM(total_amount) AS total_revenue,
        MIN(created_at) AS first_order_date,
        MAX(created_at) AS last_order_date,
        AVG(total_amount) AS avg_order_value
    FROM orders
    WHERE status = 'completed'
    GROUP BY user_id
),

customer_recency AS (
    SELECT
        user_id,
        EXTRACT(EPOCH FROM (CURRENT_TIMESTAMP - last_order_date)) / 86400 AS days_since_last_order,
        EXTRACT(EPOCH FROM (last_order_date - first_order_date)) / 86400 AS customer_lifespan_days
    FROM customer_orders
),

customer_segments AS (
    SELECT
        co.user_id,
        co.order_count,
        co.total_revenue,
        co.avg_order_value,
        cr.days_since_last_order,
        cr.customer_lifespan_days,
        CASE
            WHEN cr.days_since_last_order <= 30 AND co.order_count >= 5 THEN 'VIP'
            WHEN cr.days_since_last_order <= 90 AND co.order_count >= 2 THEN 'Active'
            WHEN cr.days_since_last_order <= 180 THEN 'At Risk'
            ELSE 'Churned'
        END AS customer_segment,
        -- Predictive LTV (simple model)
        CASE
            WHEN cr.customer_lifespan_days > 0 THEN
                (co.total_revenue / NULLIF(cr.customer_lifespan_days, 0)) * 365 * 2
            ELSE co.total_revenue
        END AS predicted_ltv
    FROM customer_orders co
    JOIN customer_recency cr ON co.user_id = cr.user_id
)

SELECT
    cs.customer_segment,
    COUNT(cs.user_id) AS customer_count,
    AVG(cs.order_count) AS avg_orders,
    AVG(cs.total_revenue) AS avg_revenue,
    AVG(cs.predicted_ltv) AS avg_predicted_ltv,
    SUM(cs.total_revenue) AS segment_revenue,
    AVG(cs.days_since_last_order) AS avg_days_since_last_order
FROM customer_segments cs
GROUP BY cs.customer_segment
ORDER BY segment_revenue DESC;
