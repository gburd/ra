-- Seven table join: full TPC-H schema traversal
SELECT r.r_name, n.n_name, c.c_name, o.o_orderdate,
       l.l_quantity, p.p_name, s.s_name
FROM region r
JOIN nation n ON r.r_regionkey = n.n_regionkey
JOIN supplier s ON n.n_nationkey = s.s_nationkey
JOIN customer c ON n.n_nationkey = c.c_nationkey
JOIN orders o ON c.c_custkey = o.o_custkey
JOIN lineitem l ON o.o_orderkey = l.l_orderkey
    AND l.l_suppkey = s.s_suppkey
JOIN part p ON l.l_partkey = p.p_partkey
WHERE r.r_name = 'ASIA'
  AND o.o_orderdate BETWEEN '1994-01-01' AND '1994-12-31'
LIMIT 100;
