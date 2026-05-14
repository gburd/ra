-- CTE referenced multiple times
WITH order_stats AS (
    SELECT o_custkey, COUNT(*) AS cnt, SUM(o_totalprice) AS total
    FROM orders
    GROUP BY o_custkey
)
SELECT
    (SELECT COUNT(*) FROM order_stats WHERE cnt > 10) AS active_customers,
    (SELECT AVG(total) FROM order_stats) AS avg_lifetime_value,
    (SELECT MAX(total) FROM order_stats) AS max_lifetime_value;
