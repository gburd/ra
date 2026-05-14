-- GROUP BY with multiple columns
SELECT l_returnflag, l_linestatus,
       SUM(l_quantity) AS sum_qty,
       SUM(l_extendedprice) AS sum_base_price,
       COUNT(*) AS count_order
FROM lineitem
WHERE l_shipdate <= '1998-09-01'
GROUP BY l_returnflag, l_linestatus
ORDER BY l_returnflag, l_linestatus;
