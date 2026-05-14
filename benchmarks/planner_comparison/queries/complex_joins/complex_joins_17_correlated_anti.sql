-- Correlated anti-join: suppliers not supplying any parts cheaper than average
SELECT s.s_suppkey, s.s_name
FROM supplier s
WHERE NOT EXISTS (
    SELECT 1 FROM partsupp ps
    WHERE ps.ps_suppkey = s.s_suppkey
      AND ps.ps_supplycost < (
          SELECT AVG(ps2.ps_supplycost) FROM partsupp ps2
          WHERE ps2.ps_partkey = ps.ps_partkey
      )
);
