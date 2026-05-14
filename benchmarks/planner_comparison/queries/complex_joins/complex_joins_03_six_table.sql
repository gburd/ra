-- Six table join across full schema
SELECT c.c_name, o.o_orderdate, l.l_extendedprice,
       s.s_name, n.n_name, p.p_name
FROM customer c
JOIN orders o ON c.c_custkey = o.o_custkey
JOIN lineitem l ON o.o_orderkey = l.l_orderkey
JOIN supplier s ON l.l_suppkey = s.s_suppkey
JOIN nation n ON s.s_nationkey = n.n_nationkey
JOIN part p ON l.l_partkey = p.p_partkey
WHERE o.o_orderdate >= '1994-01-01'
  AND o.o_orderdate < '1995-01-01';
