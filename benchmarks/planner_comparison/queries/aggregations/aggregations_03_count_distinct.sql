-- COUNT DISTINCT on multiple columns
SELECT l_shipmode,
       COUNT(DISTINCT l_orderkey) AS distinct_orders,
       COUNT(DISTINCT l_suppkey) AS distinct_suppliers,
       COUNT(*) AS total_items
FROM lineitem
WHERE l_shipdate >= '1994-01-01'
  AND l_shipdate < '1995-01-01'
GROUP BY l_shipmode;
