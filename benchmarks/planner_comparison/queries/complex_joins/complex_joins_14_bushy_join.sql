-- Bushy join: independent subqueries joined together
SELECT cust_summary.c_name, supp_summary.s_name, cust_summary.order_count
FROM (
    SELECT c.c_custkey, c.c_name, COUNT(o.o_orderkey) AS order_count
    FROM customer c
    JOIN orders o ON c.c_custkey = o.o_custkey
    GROUP BY c.c_custkey, c.c_name
) cust_summary
JOIN (
    SELECT s.s_suppkey, s.s_name, s.s_nationkey
    FROM supplier s
    WHERE s.s_acctbal > 5000
) supp_summary ON cust_summary.c_custkey = supp_summary.s_suppkey
WHERE cust_summary.order_count > 10;
