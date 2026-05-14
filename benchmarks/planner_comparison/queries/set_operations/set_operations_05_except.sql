-- EXCEPT: parts that are supplied but never ordered
SELECT ps_partkey AS partkey
FROM partsupp
EXCEPT
SELECT l_partkey AS partkey
FROM lineitem;
