-- GROUP BY ROLLUP for hierarchical aggregation
SELECT n.n_name, EXTRACT(YEAR FROM o.o_orderdate) AS year,
       SUM(o.o_totalprice) AS total_sales
FROM orders o
JOIN customer c ON o.o_custkey = c.c_custkey
JOIN nation n ON c.c_nationkey = n.n_nationkey
WHERE o.o_orderdate >= '1995-01-01'
GROUP BY ROLLUP(n.n_name, EXTRACT(YEAR FROM o.o_orderdate));
