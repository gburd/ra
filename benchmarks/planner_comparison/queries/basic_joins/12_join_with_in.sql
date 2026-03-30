-- Join with IN clause
SELECT c.c_name, o.o_orderdate
FROM customer c
JOIN orders o ON c.c_custkey = o.o_custkey
WHERE c.c_nationkey IN (1, 5, 10);
