-- Set operation on joined results
SELECT c.c_name, n.n_name, 'high_balance' AS category
FROM customer c
JOIN nation n ON c.c_nationkey = n.n_nationkey
WHERE c.c_acctbal > 9000
UNION ALL
SELECT s.s_name, n.n_name, 'active_supplier' AS category
FROM supplier s
JOIN nation n ON s.s_nationkey = n.n_nationkey
WHERE s.s_acctbal > 8000;
