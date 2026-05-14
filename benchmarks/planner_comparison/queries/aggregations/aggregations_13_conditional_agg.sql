-- Conditional aggregation with CASE
SELECT o_orderpriority,
       COUNT(*) AS total_orders,
       SUM(CASE WHEN o_orderstatus = 'F' THEN 1 ELSE 0 END) AS fulfilled,
       SUM(CASE WHEN o_orderstatus = 'O' THEN 1 ELSE 0 END) AS open,
       SUM(CASE WHEN o_orderstatus = 'P' THEN 1 ELSE 0 END) AS partial
FROM orders
GROUP BY o_orderpriority
ORDER BY o_orderpriority;
