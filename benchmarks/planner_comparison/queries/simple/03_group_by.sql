-- GROUP BY with aggregates
SELECT l_returnflag, l_linestatus,
       COUNT(*) as count,
       SUM(l_quantity) as sum_qty,
       AVG(l_extendedprice) as avg_price
FROM lineitem
GROUP BY l_returnflag, l_linestatus;
