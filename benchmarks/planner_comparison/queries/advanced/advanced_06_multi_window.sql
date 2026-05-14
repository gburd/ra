-- Multiple window functions in same query with different partitions
SELECT l_orderkey, l_partkey, l_suppkey, l_extendedprice,
       SUM(l_extendedprice) OVER (PARTITION BY l_orderkey) AS order_total,
       SUM(l_extendedprice) OVER (PARTITION BY l_suppkey) AS supplier_total,
       ROW_NUMBER() OVER (PARTITION BY l_orderkey ORDER BY l_extendedprice DESC) AS item_rank,
       COUNT(*) OVER (PARTITION BY l_partkey) AS part_frequency
FROM lineitem
WHERE l_shipdate >= '1997-01-01' AND l_shipdate < '1997-02-01';
