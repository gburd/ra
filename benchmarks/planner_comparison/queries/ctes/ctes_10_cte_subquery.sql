-- CTE combined with subquery
WITH regional_suppliers AS (
    SELECT s.s_suppkey, s.s_name, n.n_name
    FROM supplier s
    JOIN nation n ON s.s_nationkey = n.n_nationkey
    JOIN region r ON n.n_regionkey = r.r_regionkey
    WHERE r.r_name = 'EUROPE'
)
SELECT rs.s_name, rs.n_name, ps.ps_partkey, ps.ps_supplycost
FROM regional_suppliers rs
JOIN partsupp ps ON rs.s_suppkey = ps.ps_suppkey
WHERE ps.ps_supplycost = (
    SELECT MIN(ps2.ps_supplycost)
    FROM partsupp ps2
    JOIN regional_suppliers rs2 ON ps2.ps_suppkey = rs2.s_suppkey
    WHERE ps2.ps_partkey = ps.ps_partkey
)
ORDER BY ps.ps_supplycost
LIMIT 50;
