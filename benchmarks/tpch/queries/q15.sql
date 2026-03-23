SELECT s_suppkey, s_name, s_address, s_phone,
       SUM(l_extendedprice * (1 - l_discount)) AS total_revenue
FROM supplier, lineitem
WHERE s_suppkey = l_suppkey
  AND l_shipdate >= '1996-01-01'
  AND l_shipdate < '1996-04-01'
GROUP BY s_suppkey, s_name, s_address, s_phone
ORDER BY s_suppkey;
