-- Multiple self-joins: orders with same customer on same date
SELECT o1.o_orderkey AS order1, o2.o_orderkey AS order2,
       o1.o_custkey, o1.o_orderdate
FROM orders o1
JOIN orders o2 ON o1.o_custkey = o2.o_custkey
    AND o1.o_orderdate = o2.o_orderdate
    AND o1.o_orderkey < o2.o_orderkey
WHERE o1.o_orderstatus = 'F';
