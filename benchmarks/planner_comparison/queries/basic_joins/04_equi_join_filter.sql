-- INNER JOIN with filters on both sides
SELECT l.l_orderkey, l.l_linenumber, o.o_orderdate
FROM lineitem l
JOIN orders o ON l.l_orderkey = o.o_orderkey
WHERE l.l_quantity > 40
  AND o.o_totalprice > 100000;
