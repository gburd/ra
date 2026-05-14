-- NTILE: divide customers into quartiles by balance
SELECT c_custkey, c_name, c_acctbal,
       NTILE(4) OVER (ORDER BY c_acctbal DESC) AS quartile
FROM customer
WHERE c_acctbal > 0;
