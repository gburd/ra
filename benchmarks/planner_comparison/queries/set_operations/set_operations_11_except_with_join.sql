-- EXCEPT with joins: customers who ordered but never returned
SELECT DISTINCT c.c_custkey, c.c_name
FROM customer c
JOIN orders o ON c.c_custkey = o.o_custkey
EXCEPT
SELECT DISTINCT c.c_custkey, c.c_name
FROM customer c
JOIN orders o ON c.c_custkey = o.o_custkey
JOIN lineitem l ON o.o_orderkey = l.l_orderkey
WHERE l.l_returnflag = 'R';
