SELECT c_name, c_custkey, o_orderkey, o_orderdate, o_totalprice,
       SUM(l_quantity) AS total_quantity
FROM customer, orders, lineitem
WHERE c_custkey = o_custkey
  AND o_orderkey = l_orderkey
GROUP BY c_name, c_custkey, o_orderkey, o_orderdate, o_totalprice
HAVING SUM(l_quantity) > 300
ORDER BY o_totalprice DESC, o_orderdate
LIMIT 100;
