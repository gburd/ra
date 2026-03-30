-- LEFT OUTER JOIN
SELECT c.c_custkey, c.c_name, COUNT(o.o_orderkey) as order_count
FROM customer c
LEFT JOIN orders o ON c.c_custkey = o.o_custkey
GROUP BY c.c_custkey, c.c_name;
