-- OFFSET pagination
SELECT o_orderkey, o_custkey, o_totalprice
FROM orders
ORDER BY o_orderdate
LIMIT 100 OFFSET 1000;
