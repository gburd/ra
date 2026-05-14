-- Full outer join: all parts and suppliers including unmatched
SELECT p.p_partkey, p.p_name, s.s_suppkey, s.s_name
FROM part p
FULL OUTER JOIN partsupp ps ON p.p_partkey = ps.ps_partkey
FULL OUTER JOIN supplier s ON ps.ps_suppkey = s.s_suppkey
WHERE p.p_size BETWEEN 10 AND 20
   OR s.s_acctbal > 9000;
