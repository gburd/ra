-- Filter with aggregate
SELECT l_shipmode, COUNT(*) as order_count
FROM lineitem
WHERE l_shipdate >= '1997-01-01'
  AND l_shipdate < '1998-01-01'
GROUP BY l_shipmode
ORDER BY l_shipmode;
