-- INTERSECT ALL: common part keys between lineitem and partsupp
SELECT l_partkey AS partkey
FROM lineitem
WHERE l_quantity > 30
INTERSECT ALL
SELECT ps_partkey AS partkey
FROM partsupp
WHERE ps_availqty > 5000;
