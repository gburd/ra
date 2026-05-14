-- Diamond join pattern: two paths from lineitem to nation
SELECT l.l_orderkey, cn.n_name AS customer_nation, sn.n_name AS supplier_nation,
       l.l_extendedprice
FROM lineitem l
JOIN orders o ON l.l_orderkey = o.o_orderkey
JOIN customer c ON o.o_custkey = c.c_custkey
JOIN nation cn ON c.c_nationkey = cn.n_nationkey
JOIN supplier s ON l.l_suppkey = s.s_suppkey
JOIN nation sn ON s.s_nationkey = sn.n_nationkey
WHERE cn.n_name = 'FRANCE' AND sn.n_name = 'GERMANY';
