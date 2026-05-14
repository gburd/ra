-- Multi-level aggregation: summarize per-customer aggregates by nation
SELECT n.n_name,
       COUNT(*) AS customer_count,
       AVG(cust_total) AS avg_customer_spend,
       MAX(cust_total) AS max_customer_spend
FROM nation n
JOIN (
    SELECT c.c_nationkey, SUM(o.o_totalprice) AS cust_total
    FROM customer c
    JOIN orders o ON c.c_custkey = o.o_custkey
    GROUP BY c.c_custkey, c.c_nationkey
) cust_agg ON n.n_nationkey = cust_agg.c_nationkey
GROUP BY n.n_name
ORDER BY avg_customer_spend DESC;
