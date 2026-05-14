-- Correlated scalar subquery in SELECT list
SELECT c.c_name, c.c_acctbal,
       (SELECT COUNT(*) FROM orders o WHERE o.o_custkey = c.c_custkey) AS order_count
FROM customer c
WHERE c.c_acctbal > 5000;
