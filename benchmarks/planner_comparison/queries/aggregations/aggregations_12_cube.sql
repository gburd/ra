-- GROUP BY CUBE for full dimensional aggregation
SELECT l_returnflag, l_shipmode,
       SUM(l_quantity) AS total_qty,
       COUNT(*) AS cnt
FROM lineitem
WHERE l_shipdate >= '1994-01-01' AND l_shipdate < '1995-01-01'
GROUP BY CUBE(l_returnflag, l_shipmode);
