-- Anti-join via NOT EXISTS: customers with no orders
SELECT c.c_custkey, c.c_name, c.c_acctbal
FROM customer c
WHERE NOT EXISTS (
    SELECT 1 FROM orders o
    WHERE o.o_custkey = c.c_custkey
);
