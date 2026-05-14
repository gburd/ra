-- Complex anti-join: parts never supplied below threshold by any European supplier
SELECT p.p_partkey, p.p_name, p.p_retailprice
FROM part p
WHERE NOT EXISTS (
    SELECT 1
    FROM partsupp ps
    JOIN supplier s ON ps.ps_suppkey = s.s_suppkey
    JOIN nation n ON s.s_nationkey = n.n_nationkey
    JOIN region r ON n.n_regionkey = r.r_regionkey
    WHERE ps.ps_partkey = p.p_partkey
      AND r.r_name = 'EUROPE'
      AND ps.ps_supplycost < 100
);
