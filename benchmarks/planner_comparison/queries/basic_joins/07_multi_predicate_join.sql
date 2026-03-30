-- Join with multiple predicates
SELECT p.p_partkey, ps.ps_supplycost
FROM part p
JOIN partsupp ps ON p.p_partkey = ps.ps_partkey
WHERE p.p_size > 30
  AND ps.ps_availqty > 5000;
