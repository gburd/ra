-- Aggregate with subquery filter: suppliers above average supply cost
SELECT s.s_name, agg.part_count, agg.avg_cost
FROM supplier s
JOIN (
    SELECT ps_suppkey,
           COUNT(DISTINCT ps_partkey) AS part_count,
           AVG(ps_supplycost) AS avg_cost
    FROM partsupp
    GROUP BY ps_suppkey
    HAVING AVG(ps_supplycost) > (SELECT AVG(ps_supplycost) FROM partsupp)
) agg ON s.s_suppkey = agg.ps_suppkey
ORDER BY agg.avg_cost DESC
LIMIT 20;
