-- Join with DISTINCT
SELECT DISTINCT c.c_nationkey, o.o_orderpriority
FROM customer c
JOIN orders o ON c.c_custkey = o.o_custkey;
