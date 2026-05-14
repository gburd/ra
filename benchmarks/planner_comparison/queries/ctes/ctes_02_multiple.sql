-- Multiple CTEs referencing each other
WITH high_value_orders AS (
    SELECT o_orderkey, o_custkey, o_totalprice
    FROM orders
    WHERE o_totalprice > 300000
),
high_value_customers AS (
    SELECT c.c_custkey, c.c_name, COUNT(*) AS big_order_count
    FROM customer c
    JOIN high_value_orders hvo ON c.c_custkey = hvo.o_custkey
    GROUP BY c.c_custkey, c.c_name
)
SELECT c_name, big_order_count
FROM high_value_customers
WHERE big_order_count >= 3
ORDER BY big_order_count DESC;
