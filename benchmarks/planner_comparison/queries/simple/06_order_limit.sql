-- ORDER BY with LIMIT
SELECT l_orderkey, l_partkey, l_extendedprice
FROM lineitem
ORDER BY l_extendedprice DESC
LIMIT 100;
