-- Theta join with range condition
SELECT l.l_orderkey, l.l_partkey, ps.ps_suppkey, ps.ps_supplycost
FROM lineitem l
JOIN partsupp ps ON l.l_partkey = ps.ps_partkey
    AND l.l_suppkey = ps.ps_suppkey
WHERE l.l_extendedprice > ps.ps_supplycost * 100
  AND l.l_quantity > 20;
