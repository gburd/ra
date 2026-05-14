-- Multi-table DELETE with subquery
DELETE FROM lineitem
WHERE l_orderkey IN (
    SELECT o.o_orderkey
    FROM orders o
    JOIN customer c ON o.o_custkey = c.c_custkey
    WHERE c.c_acctbal < -500
      AND o.o_orderstatus = 'O'
)
AND l_shipdate < '1993-01-01';
