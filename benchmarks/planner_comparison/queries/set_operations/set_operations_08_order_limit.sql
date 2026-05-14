-- Set operation with ORDER BY and LIMIT
SELECT c_custkey AS key, c_name AS name, c_acctbal AS balance
FROM customer
WHERE c_acctbal > 9500
UNION ALL
SELECT s_suppkey AS key, s_name AS name, s_acctbal AS balance
FROM supplier
WHERE s_acctbal > 9500
ORDER BY balance DESC
LIMIT 30;
