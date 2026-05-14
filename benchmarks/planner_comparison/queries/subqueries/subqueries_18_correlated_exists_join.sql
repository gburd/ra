-- Correlated EXISTS with inner join
SELECT c.c_name, c.c_acctbal
FROM customer c
WHERE EXISTS (
    SELECT 1
    FROM orders o
    JOIN lineitem l ON o.o_orderkey = l.l_orderkey
    WHERE o.o_custkey = c.c_custkey
      AND l.l_returnflag = 'R'
      AND l.l_extendedprice > 10000
);
