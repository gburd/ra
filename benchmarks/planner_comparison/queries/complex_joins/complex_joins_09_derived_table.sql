-- Join with derived table: top customers by revenue
SELECT c.c_name, top_orders.total_revenue
FROM customer c
JOIN (
    SELECT o.o_custkey, SUM(o.o_totalprice) AS total_revenue
    FROM orders o
    GROUP BY o.o_custkey
    HAVING SUM(o.o_totalprice) > 500000
) top_orders ON c.c_custkey = top_orders.o_custkey
ORDER BY top_orders.total_revenue DESC
LIMIT 20;
