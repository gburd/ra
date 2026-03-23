SELECT cntrycode, COUNT(*) AS numcust, SUM(c_acctbal) AS totacctbal
FROM customer
WHERE c_acctbal > 0
  AND NOT EXISTS (
    SELECT * FROM orders WHERE o_custkey = c_custkey
  )
GROUP BY cntrycode
ORDER BY cntrycode;
