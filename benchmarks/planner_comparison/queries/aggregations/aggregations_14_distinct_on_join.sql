-- COUNT DISTINCT on joined result
SELECT r.r_name,
       COUNT(DISTINCT c.c_custkey) AS distinct_customers,
       COUNT(DISTINCT o.o_orderkey) AS distinct_orders,
       SUM(o.o_totalprice) AS total_revenue
FROM region r
JOIN nation n ON r.r_regionkey = n.n_regionkey
JOIN customer c ON n.n_nationkey = c.c_nationkey
JOIN orders o ON c.c_custkey = o.o_custkey
GROUP BY r.r_name
ORDER BY total_revenue DESC;
