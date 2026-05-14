-- CTE with window function
WITH ranked_orders AS (
    SELECT o_custkey, o_orderkey, o_totalprice,
           ROW_NUMBER() OVER (PARTITION BY o_custkey ORDER BY o_totalprice DESC) AS rn
    FROM orders
)
SELECT c.c_name, ro.o_orderkey, ro.o_totalprice
FROM ranked_orders ro
JOIN customer c ON ro.o_custkey = c.c_custkey
WHERE ro.rn <= 3
ORDER BY c.c_name, ro.rn;
