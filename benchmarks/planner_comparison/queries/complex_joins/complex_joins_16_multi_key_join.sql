-- Multi-key join with composite condition
SELECT l.l_orderkey, l.l_linenumber, ps.ps_availqty, ps.ps_supplycost
FROM lineitem l
JOIN partsupp ps ON l.l_partkey = ps.ps_partkey
    AND l.l_suppkey = ps.ps_suppkey
JOIN orders o ON l.l_orderkey = o.o_orderkey
WHERE o.o_orderstatus = 'O'
  AND ps.ps_availqty > l.l_quantity;
