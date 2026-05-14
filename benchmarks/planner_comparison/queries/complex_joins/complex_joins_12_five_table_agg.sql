-- Five table join with aggregation: revenue by region and year
SELECT r.r_name, EXTRACT(YEAR FROM o.o_orderdate) AS order_year,
       SUM(l.l_extendedprice * (1 - l.l_discount)) AS revenue
FROM region r
JOIN nation n ON r.r_regionkey = n.n_regionkey
JOIN customer c ON n.n_nationkey = c.c_nationkey
JOIN orders o ON c.c_custkey = o.o_custkey
JOIN lineitem l ON o.o_orderkey = l.l_orderkey
WHERE o.o_orderdate >= '1993-01-01'
  AND o.o_orderdate < '1998-01-01'
GROUP BY r.r_name, EXTRACT(YEAR FROM o.o_orderdate)
ORDER BY r.r_name, order_year;
