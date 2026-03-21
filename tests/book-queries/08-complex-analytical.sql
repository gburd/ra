-- Complex analytical queries
-- Source: "SQL Performance Explained", "Designing Data-Intensive Applications", "High Performance MySQL"

-- Running total with window function
SELECT
    order_date,
    order_id,
    amount,
    SUM(amount) OVER (ORDER BY order_date, order_id) AS running_total
FROM orders;

-- Year-over-year comparison
WITH sales_by_year AS (
    SELECT
        EXTRACT(YEAR FROM order_date) AS year,
        SUM(amount) AS total_sales
    FROM orders
    GROUP BY EXTRACT(YEAR FROM order_date)
)
SELECT
    curr.year,
    curr.total_sales,
    prev.total_sales AS prev_year_sales,
    curr.total_sales - prev.total_sales AS yoy_change,
    ROUND(100.0 * (curr.total_sales - prev.total_sales) / prev.total_sales, 2) AS yoy_pct_change
FROM sales_by_year curr
LEFT JOIN sales_by_year prev ON curr.year = prev.year + 1;

-- Cohort analysis
WITH first_orders AS (
    SELECT
        customer_id,
        MIN(order_date) AS first_order_date
    FROM orders
    GROUP BY customer_id
),
cohort_data AS (
    SELECT
        DATE_TRUNC('month', fo.first_order_date) AS cohort_month,
        DATE_TRUNC('month', o.order_date) AS order_month,
        COUNT(DISTINCT o.customer_id) AS customer_count
    FROM orders o
    JOIN first_orders fo ON o.customer_id = fo.customer_id
    GROUP BY cohort_month, order_month
)
SELECT * FROM cohort_data ORDER BY cohort_month, order_month;

-- Moving average (3-month)
SELECT
    DATE_TRUNC('month', order_date) AS month,
    SUM(amount) AS monthly_sales,
    AVG(SUM(amount)) OVER (
        ORDER BY DATE_TRUNC('month', order_date)
        ROWS BETWEEN 2 PRECEDING AND CURRENT ROW
    ) AS moving_avg_3m
FROM orders
GROUP BY DATE_TRUNC('month', order_date);

-- Percentile calculation
SELECT
    department_id,
    PERCENTILE_CONT(0.25) WITHIN GROUP (ORDER BY salary) AS p25,
    PERCENTILE_CONT(0.50) WITHIN GROUP (ORDER BY salary) AS median,
    PERCENTILE_CONT(0.75) WITHIN GROUP (ORDER BY salary) AS p75,
    PERCENTILE_CONT(0.90) WITHIN GROUP (ORDER BY salary) AS p90
FROM employees
GROUP BY department_id;

-- Customer RFM (Recency, Frequency, Monetary) analysis
WITH customer_metrics AS (
    SELECT
        customer_id,
        MAX(order_date) AS last_order_date,
        COUNT(*) AS order_count,
        SUM(amount) AS total_spent
    FROM orders
    GROUP BY customer_id
),
rfm_scores AS (
    SELECT
        customer_id,
        CURRENT_DATE - last_order_date AS recency_days,
        order_count AS frequency,
        total_spent AS monetary,
        NTILE(5) OVER (ORDER BY CURRENT_DATE - last_order_date DESC) AS recency_score,
        NTILE(5) OVER (ORDER BY order_count) AS frequency_score,
        NTILE(5) OVER (ORDER BY total_spent) AS monetary_score
    FROM customer_metrics
)
SELECT
    customer_id,
    recency_score,
    frequency_score,
    monetary_score,
    (recency_score + frequency_score + monetary_score) / 3.0 AS avg_score
FROM rfm_scores;

-- Gap and island problem (find consecutive sequences)
WITH numbered_rows AS (
    SELECT
        employee_id,
        hire_date,
        ROW_NUMBER() OVER (ORDER BY hire_date) AS rn,
        hire_date - INTERVAL '1 day' * ROW_NUMBER() OVER (ORDER BY hire_date) AS grp
    FROM employees
)
SELECT
    MIN(hire_date) AS sequence_start,
    MAX(hire_date) AS sequence_end,
    COUNT(*) AS sequence_length
FROM numbered_rows
GROUP BY grp
ORDER BY sequence_start;

-- Funnel analysis
WITH funnel_steps AS (
    SELECT
        'Page View' AS step,
        1 AS step_order,
        COUNT(DISTINCT user_id) AS user_count
    FROM page_views
    UNION ALL
    SELECT
        'Add to Cart' AS step,
        2 AS step_order,
        COUNT(DISTINCT user_id) AS user_count
    FROM cart_additions
    UNION ALL
    SELECT
        'Checkout' AS step,
        3 AS step_order,
        COUNT(DISTINCT user_id) AS user_count
    FROM checkouts
    UNION ALL
    SELECT
        'Purchase' AS step,
        4 AS step_order,
        COUNT(DISTINCT user_id) AS user_count
    FROM purchases
)
SELECT
    step,
    user_count,
    LAG(user_count) OVER (ORDER BY step_order) AS prev_step_count,
    ROUND(100.0 * user_count / LAG(user_count) OVER (ORDER BY step_order), 2) AS conversion_rate
FROM funnel_steps
ORDER BY step_order;

-- Session window (group events into sessions with timeout)
WITH event_with_lag AS (
    SELECT
        user_id,
        event_time,
        LAG(event_time) OVER (PARTITION BY user_id ORDER BY event_time) AS prev_event_time
    FROM user_events
),
session_starts AS (
    SELECT
        user_id,
        event_time,
        CASE
            WHEN prev_event_time IS NULL THEN 1
            WHEN EXTRACT(EPOCH FROM (event_time - prev_event_time)) > 1800 THEN 1
            ELSE 0
        END AS is_session_start
    FROM event_with_lag
),
sessions AS (
    SELECT
        user_id,
        event_time,
        SUM(is_session_start) OVER (PARTITION BY user_id ORDER BY event_time) AS session_id
    FROM session_starts
)
SELECT
    user_id,
    session_id,
    MIN(event_time) AS session_start,
    MAX(event_time) AS session_end,
    COUNT(*) AS event_count
FROM sessions
GROUP BY user_id, session_id;

-- Top N per group
WITH ranked AS (
    SELECT
        department_id,
        first_name,
        last_name,
        salary,
        ROW_NUMBER() OVER (PARTITION BY department_id ORDER BY salary DESC) AS rn
    FROM employees
)
SELECT * FROM ranked WHERE rn <= 3;

-- Cumulative distribution
SELECT
    salary,
    COUNT(*) AS frequency,
    SUM(COUNT(*)) OVER (ORDER BY salary) AS cumulative_count,
    ROUND(100.0 * SUM(COUNT(*)) OVER (ORDER BY salary) / SUM(COUNT(*)) OVER (), 2) AS cumulative_pct
FROM employees
GROUP BY salary
ORDER BY salary;

-- Market basket analysis (products bought together)
SELECT
    o1.product_id AS product_a,
    o2.product_id AS product_b,
    COUNT(*) AS times_bought_together
FROM order_items o1
JOIN order_items o2 ON o1.order_id = o2.order_id AND o1.product_id < o2.product_id
GROUP BY o1.product_id, o2.product_id
HAVING COUNT(*) > 10
ORDER BY times_bought_together DESC;

-- Churn analysis
WITH monthly_activity AS (
    SELECT
        customer_id,
        DATE_TRUNC('month', order_date) AS month
    FROM orders
    GROUP BY customer_id, DATE_TRUNC('month', order_date)
),
customer_months AS (
    SELECT
        customer_id,
        month,
        LEAD(month) OVER (PARTITION BY customer_id ORDER BY month) AS next_month
    FROM monthly_activity
)
SELECT
    month,
    COUNT(*) AS active_customers,
    COUNT(CASE WHEN next_month IS NULL OR next_month > month + INTERVAL '1 month' THEN 1 END) AS churned_customers,
    ROUND(100.0 * COUNT(CASE WHEN next_month IS NULL OR next_month > month + INTERVAL '1 month' THEN 1 END) / COUNT(*), 2) AS churn_rate
FROM customer_months
GROUP BY month
ORDER BY month;
