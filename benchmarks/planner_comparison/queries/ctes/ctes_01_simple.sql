-- Simple CTE: customer order summary
WITH customer_orders AS (
    SELECT c_custkey, c_name, COUNT(*) AS order_count
    FROM customer
    JOIN orders ON c_custkey = o_custkey
    GROUP BY c_custkey, c_name
)
SELECT c_name, order_count
FROM customer_orders
WHERE order_count > 20
ORDER BY order_count DESC;
