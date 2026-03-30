-- RIGHT OUTER JOIN
SELECT o.o_orderkey, c.c_name
FROM orders o
RIGHT JOIN customer c ON o.o_custkey = c.c_custkey;
