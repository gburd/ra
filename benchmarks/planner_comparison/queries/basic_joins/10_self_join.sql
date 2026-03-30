-- Self join
SELECT l1.l_orderkey, l1.l_partkey, l2.l_partkey
FROM lineitem l1
JOIN lineitem l2 ON l1.l_orderkey = l2.l_orderkey
WHERE l1.l_linenumber < l2.l_linenumber
LIMIT 1000;
