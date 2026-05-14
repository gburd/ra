-- FILTER clause on aggregates
SELECT l_shipmode,
       COUNT(*) AS total_items,
       COUNT(*) FILTER (WHERE l_returnflag = 'R') AS returned_items,
       SUM(l_extendedprice) FILTER (WHERE l_discount > 0.05) AS discounted_revenue,
       AVG(l_quantity) FILTER (WHERE l_linestatus = 'F') AS avg_fulfilled_qty
FROM lineitem
GROUP BY l_shipmode
ORDER BY l_shipmode;
