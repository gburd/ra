-- Non-equi join
SELECT l1.l_orderkey, l1.l_quantity, l2.l_quantity
FROM lineitem l1
JOIN lineitem l2 ON l1.l_orderkey = l2.l_orderkey
  AND l1.l_quantity < l2.l_quantity
WHERE l1.l_linenumber = 1
LIMIT 100;
