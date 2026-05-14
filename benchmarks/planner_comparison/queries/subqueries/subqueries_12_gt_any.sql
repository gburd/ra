-- Comparison subquery: > ANY
SELECT s.s_suppkey, s.s_name, s.s_acctbal
FROM supplier s
WHERE s.s_acctbal > ANY (
    SELECT c.c_acctbal FROM customer c
    WHERE c.c_nationkey = s.s_nationkey
);
