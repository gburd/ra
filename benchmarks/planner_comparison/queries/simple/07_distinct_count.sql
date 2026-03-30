-- DISTINCT aggregation
SELECT COUNT(DISTINCT l_partkey) as distinct_parts
FROM lineitem;
