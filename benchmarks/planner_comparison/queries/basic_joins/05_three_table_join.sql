-- Three-table star join
SELECT c.c_name, o.o_orderkey, l.l_quantity
FROM customer c
JOIN orders o ON c.c_custkey = o.o_custkey
JOIN lineitem l ON o.o_orderkey = l.l_orderkey
WHERE c.c_nationkey = 5;
