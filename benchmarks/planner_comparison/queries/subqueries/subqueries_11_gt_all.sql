-- Comparison subquery: > ALL
SELECT p.p_partkey, p.p_name, p.p_retailprice
FROM part p
WHERE p.p_retailprice > ALL (
    SELECT ps.ps_supplycost FROM partsupp ps
    WHERE ps.ps_partkey = p.p_partkey
);
