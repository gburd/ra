SELECT nation, o_year, SUM(amount) AS sum_profit
FROM part, supplier, lineitem, partsupp, orders, nation
WHERE s_suppkey = l_suppkey
  AND ps_suppkey = l_suppkey
  AND ps_partkey = l_partkey
  AND p_partkey = l_partkey
  AND o_orderkey = l_orderkey
  AND s_nationkey = n_nationkey
  AND p_name = 'green'
GROUP BY nation, o_year
ORDER BY nation, o_year DESC;
