-- HAVING clause
SELECT l_returnflag, COUNT(*) as count
FROM lineitem
GROUP BY l_returnflag
HAVING COUNT(*) > 1000000;
