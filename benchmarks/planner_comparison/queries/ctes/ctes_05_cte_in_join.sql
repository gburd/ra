-- CTE used in JOIN
WITH top_suppliers AS (
    SELECT ps_suppkey, SUM(ps_availqty) AS total_inventory
    FROM partsupp
    GROUP BY ps_suppkey
    HAVING SUM(ps_availqty) > 10000
)
SELECT s.s_name, n.n_name, ts.total_inventory
FROM top_suppliers ts
JOIN supplier s ON ts.ps_suppkey = s.s_suppkey
JOIN nation n ON s.s_nationkey = n.n_nationkey
ORDER BY ts.total_inventory DESC
LIMIT 20;
