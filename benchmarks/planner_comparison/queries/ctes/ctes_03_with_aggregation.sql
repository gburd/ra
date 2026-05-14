-- CTE with aggregation used in comparison
WITH avg_by_nation AS (
    SELECT c.c_nationkey, AVG(o.o_totalprice) AS avg_order_value
    FROM customer c
    JOIN orders o ON c.c_custkey = o.o_custkey
    GROUP BY c.c_nationkey
)
SELECT n.n_name, abn.avg_order_value
FROM avg_by_nation abn
JOIN nation n ON abn.c_nationkey = n.n_nationkey
WHERE abn.avg_order_value > (SELECT AVG(avg_order_value) FROM avg_by_nation)
ORDER BY abn.avg_order_value DESC;
