-- Recursive CTE with depth-limited expansion
WITH RECURSIVE supply_chain AS (
    SELECT ps_partkey AS part, ps_suppkey AS supplier, 1 AS depth
    FROM partsupp
    WHERE ps_supplycost < 50
    UNION ALL
    SELECT sc.part, ps.ps_suppkey, sc.depth + 1
    FROM supply_chain sc
    JOIN partsupp ps ON sc.supplier = ps.ps_partkey
    WHERE sc.depth < 3
)
SELECT part, COUNT(DISTINCT supplier) AS reachable_suppliers, MAX(depth) AS max_depth
FROM supply_chain
GROUP BY part
HAVING COUNT(DISTINCT supplier) > 5
ORDER BY reachable_suppliers DESC
LIMIT 20;
