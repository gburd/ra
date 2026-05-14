-- EXISTS subquery: orders with high-value line items
SELECT o.o_orderkey, o.o_orderdate, o.o_totalprice
FROM orders o
WHERE EXISTS (
    SELECT 1 FROM lineitem l
    WHERE l.l_orderkey = o.o_orderkey
      AND l.l_extendedprice > 50000
);
