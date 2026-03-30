-- Simple INNER JOIN
SELECT o.o_orderkey, o.o_orderdate, c.c_name
FROM orders o
INNER JOIN customer c ON o.o_custkey = c.c_custkey
WHERE o.o_orderdate >= '1998-01-01';
