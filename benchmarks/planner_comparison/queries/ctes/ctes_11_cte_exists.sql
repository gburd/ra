-- CTE used in EXISTS predicate
WITH large_orders AS (
    SELECT o_orderkey, o_custkey
    FROM orders
    WHERE o_totalprice > 200000
)
SELECT c.c_custkey, c.c_name, c.c_acctbal
FROM customer c
WHERE EXISTS (
    SELECT 1 FROM large_orders lo
    WHERE lo.o_custkey = c.c_custkey
)
AND c.c_acctbal > 0
ORDER BY c.c_acctbal DESC;
