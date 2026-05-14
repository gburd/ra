-- Lateral join with aggregate
SELECT s.s_name, part_stats.part_count, part_stats.min_cost
FROM supplier s,
LATERAL (
    SELECT COUNT(*) AS part_count, MIN(ps.ps_supplycost) AS min_cost
    FROM partsupp ps
    WHERE ps.ps_suppkey = s.s_suppkey
) part_stats
WHERE part_stats.part_count > 50;
