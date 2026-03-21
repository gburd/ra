-- dbt Model: User Funnel Analysis
-- Source: Product analytics, conversion tracking
-- Pattern: OLAP - Funnel metrics with sessionization

WITH funnel_events AS (
    SELECT
        user_id,
        session_id,
        event_type,
        event_timestamp,
        ROW_NUMBER() OVER (
            PARTITION BY user_id, session_id
            ORDER BY event_timestamp
        ) AS event_sequence
    FROM raw_events
    WHERE event_timestamp >= CURRENT_DATE - INTERVAL '7 days'
        AND event_type IN (
            'landing_page',
            'product_view',
            'add_to_cart',
            'checkout_start',
            'payment_info',
            'purchase_complete'
        )
),

session_funnel AS (
    SELECT
        session_id,
        user_id,
        MAX(CASE WHEN event_type = 'landing_page' THEN 1 ELSE 0 END) AS reached_landing,
        MAX(CASE WHEN event_type = 'product_view' THEN 1 ELSE 0 END) AS reached_product,
        MAX(CASE WHEN event_type = 'add_to_cart' THEN 1 ELSE 0 END) AS reached_cart,
        MAX(CASE WHEN event_type = 'checkout_start' THEN 1 ELSE 0 END) AS reached_checkout,
        MAX(CASE WHEN event_type = 'payment_info' THEN 1 ELSE 0 END) AS reached_payment,
        MAX(CASE WHEN event_type = 'purchase_complete' THEN 1 ELSE 0 END) AS reached_purchase,
        MIN(event_timestamp) AS session_start,
        MAX(event_timestamp) AS session_end
    FROM funnel_events
    GROUP BY session_id, user_id
),

funnel_metrics AS (
    SELECT
        COUNT(*) AS total_sessions,
        SUM(reached_landing) AS landing_count,
        SUM(reached_product) AS product_count,
        SUM(reached_cart) AS cart_count,
        SUM(reached_checkout) AS checkout_count,
        SUM(reached_payment) AS payment_count,
        SUM(reached_purchase) AS purchase_count
    FROM session_funnel
)

SELECT
    'Landing Page' AS step,
    1 AS step_order,
    landing_count AS user_count,
    100.0 AS conversion_rate,
    0.0 AS drop_off_rate
FROM funnel_metrics

UNION ALL

SELECT
    'Product View' AS step,
    2 AS step_order,
    product_count AS user_count,
    (product_count::DECIMAL / NULLIF(landing_count, 0)) * 100 AS conversion_rate,
    ((landing_count - product_count)::DECIMAL / NULLIF(landing_count, 0)) * 100 AS drop_off_rate
FROM funnel_metrics

UNION ALL

SELECT
    'Add to Cart' AS step,
    3 AS step_order,
    cart_count AS user_count,
    (cart_count::DECIMAL / NULLIF(product_count, 0)) * 100 AS conversion_rate,
    ((product_count - cart_count)::DECIMAL / NULLIF(product_count, 0)) * 100 AS drop_off_rate
FROM funnel_metrics

UNION ALL

SELECT
    'Checkout Start' AS step,
    4 AS step_order,
    checkout_count AS user_count,
    (checkout_count::DECIMAL / NULLIF(cart_count, 0)) * 100 AS conversion_rate,
    ((cart_count - checkout_count)::DECIMAL / NULLIF(cart_count, 0)) * 100 AS drop_off_rate
FROM funnel_metrics

UNION ALL

SELECT
    'Payment Info' AS step,
    5 AS step_order,
    payment_count AS user_count,
    (payment_count::DECIMAL / NULLIF(checkout_count, 0)) * 100 AS conversion_rate,
    ((checkout_count - payment_count)::DECIMAL / NULLIF(checkout_count, 0)) * 100 AS drop_off_rate
FROM funnel_metrics

UNION ALL

SELECT
    'Purchase Complete' AS step,
    6 AS step_order,
    purchase_count AS user_count,
    (purchase_count::DECIMAL / NULLIF(payment_count, 0)) * 100 AS conversion_rate,
    ((payment_count - purchase_count)::DECIMAL / NULLIF(payment_count, 0)) * 100 AS drop_off_rate
FROM funnel_metrics

ORDER BY step_order;
