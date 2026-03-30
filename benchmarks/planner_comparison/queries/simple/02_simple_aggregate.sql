-- Simple aggregation
SELECT COUNT(*), SUM(l_quantity), AVG(l_extendedprice)
FROM lineitem;
