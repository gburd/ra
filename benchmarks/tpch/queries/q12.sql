SELECT l_shipmode, COUNT(*) AS order_count, COUNT(*) AS late_count
FROM orders, lineitem
WHERE o_orderkey = l_orderkey
  AND (l_shipmode = 'MAIL' OR l_shipmode = 'SHIP')
  AND l_commitdate < l_receiptdate
  AND l_shipdate < l_commitdate
  AND l_receiptdate >= '1994-01-01'
  AND l_receiptdate < '1995-01-01'
GROUP BY l_shipmode
ORDER BY l_shipmode;
