-- HAVING clause filtering aggregate results
SELECT o_custkey, COUNT(*) AS order_count, SUM(o_totalprice) AS total_spent
FROM orders
GROUP BY o_custkey
HAVING COUNT(*) > 10
ORDER BY total_spent DESC;
