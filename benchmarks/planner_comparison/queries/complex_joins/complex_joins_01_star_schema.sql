-- Star schema join: fact table with 4 dimensions
SELECT c.c_name, n.n_name, r.r_name, o.o_orderdate, l.l_extendedprice
FROM lineitem l
JOIN orders o ON l.l_orderkey = o.o_orderkey
JOIN customer c ON o.o_custkey = c.c_custkey
JOIN nation n ON c.c_nationkey = n.n_nationkey
JOIN region r ON n.n_regionkey = r.r_regionkey
WHERE l.l_shipdate >= '1995-01-01'
  AND l.l_shipdate < '1996-01-01';
