-- Recursive CTE: region hierarchy traversal
WITH RECURSIVE region_tree AS (
    SELECT r_regionkey, r_name, 0 AS depth
    FROM region
    WHERE r_regionkey = 1
    UNION ALL
    SELECT n.n_nationkey, n.n_name, rt.depth + 1
    FROM nation n
    JOIN region_tree rt ON n.n_regionkey = rt.r_regionkey
    WHERE rt.depth < 2
)
SELECT r_regionkey, r_name, depth
FROM region_tree
ORDER BY depth, r_name;
