-- Correlated subquery with aggregate: parts above supplier average cost
SELECT ps.ps_partkey, ps.ps_suppkey, ps.ps_supplycost
FROM partsupp ps
WHERE ps.ps_supplycost > (
    SELECT AVG(ps2.ps_supplycost)
    FROM partsupp ps2
    WHERE ps2.ps_suppkey = ps.ps_suppkey
);
