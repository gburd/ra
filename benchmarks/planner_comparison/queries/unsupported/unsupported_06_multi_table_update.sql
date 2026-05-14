-- Multi-table UPDATE with FROM clause
UPDATE orders
SET o_orderstatus = 'X'
FROM customer c
JOIN nation n ON c.c_nationkey = n.n_nationkey
WHERE orders.o_custkey = c.c_custkey
  AND n.n_name = 'GERMANY'
  AND orders.o_totalprice < 1000;
