-- Join with aggregation
SELECT o.o_orderpriority, COUNT(*) as order_count
FROM orders o
JOIN lineitem l ON o.o_orderkey = l.l_orderkey
WHERE l.l_commitdate < l.l_receiptdate
GROUP BY o.o_orderpriority;
