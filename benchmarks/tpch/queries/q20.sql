SELECT s_name, s_address
FROM supplier, nation
WHERE s_suppkey IN (
    SELECT ps_suppkey
    FROM partsupp
    WHERE ps_partkey IN (
      SELECT l_partkey FROM lineitem
      WHERE l_shipdate >= '1994-01-01'
        AND l_shipdate < '1995-01-01'
    )
  )
  AND s_nationkey = n_nationkey
  AND n_name = 'CANADA'
ORDER BY s_name;
