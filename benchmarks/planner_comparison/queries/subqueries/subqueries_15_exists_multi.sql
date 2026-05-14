-- Multiple EXISTS conditions
SELECT c.c_custkey, c.c_name
FROM customer c
WHERE EXISTS (
    SELECT 1 FROM orders o
    WHERE o.o_custkey = c.c_custkey
      AND o.o_orderstatus = 'F'
)
AND EXISTS (
    SELECT 1 FROM orders o
    WHERE o.o_custkey = c.c_custkey
      AND o.o_orderstatus = 'O'
);
