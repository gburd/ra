-- Aggregate with derived percentile calculation
SELECT l_shipmode,
       COUNT(*) AS cnt,
       SUM(CASE WHEN l_quantity > 30 THEN 1 ELSE 0 END) AS high_qty_count,
       AVG(l_extendedprice) AS avg_price,
       SUM(l_extendedprice) AS total_revenue
FROM lineitem
GROUP BY l_shipmode
ORDER BY total_revenue DESC;
