-- Subquery in HAVING clause
SELECT l_partkey, SUM(l_quantity) AS total_qty
FROM lineitem
GROUP BY l_partkey
HAVING SUM(l_quantity) > (
    SELECT AVG(total_qty) FROM (
        SELECT l_partkey, SUM(l_quantity) AS total_qty
        FROM lineitem
        GROUP BY l_partkey
    ) part_totals
);
