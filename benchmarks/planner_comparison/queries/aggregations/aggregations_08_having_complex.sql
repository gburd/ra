-- Complex HAVING with multiple conditions
SELECT l_partkey,
       COUNT(*) AS order_count,
       SUM(l_quantity) AS total_qty,
       AVG(l_extendedprice) AS avg_price
FROM lineitem
WHERE l_shipdate >= '1995-01-01'
GROUP BY l_partkey
HAVING COUNT(*) > 5
   AND SUM(l_quantity) > 100
   AND AVG(l_extendedprice) > 1000
ORDER BY order_count DESC
LIMIT 50;
