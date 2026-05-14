-- Full outer join with complex non-equi conditions and COALESCE
SELECT COALESCE(c.c_custkey, s.s_suppkey) AS entity_key,
       COALESCE(c.c_name, s.s_name) AS entity_name,
       c.c_acctbal AS cust_balance,
       s.s_acctbal AS supp_balance
FROM customer c
FULL OUTER JOIN supplier s ON c.c_nationkey = s.s_nationkey
    AND c.c_acctbal BETWEEN s.s_acctbal - 1000 AND s.s_acctbal + 1000
WHERE COALESCE(c.c_acctbal, 0) + COALESCE(s.s_acctbal, 0) > 15000;
