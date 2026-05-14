-- RANK and DENSE_RANK: supplier ranking by account balance
SELECT s_suppkey, s_name, s_acctbal, s_nationkey,
       RANK() OVER (PARTITION BY s_nationkey ORDER BY s_acctbal DESC) AS rank_bal,
       DENSE_RANK() OVER (PARTITION BY s_nationkey ORDER BY s_acctbal DESC) AS dense_rank_bal
FROM supplier;
