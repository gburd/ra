SELECT supp_nation, cust_nation, l_year,
       SUM(l_extendedprice * (1 - l_discount)) AS revenue
FROM supplier, lineitem, orders, customer, nation n1, nation n2
WHERE s_suppkey = l_suppkey
  AND o_orderkey = l_orderkey
  AND c_custkey = o_custkey
  AND s_nationkey = n1.n_nationkey
  AND c_nationkey = n2.n_nationkey
  AND (n1.n_name = 'FRANCE' OR n1.n_name = 'GERMANY')
  AND (n2.n_name = 'FRANCE' OR n2.n_name = 'GERMANY')
  AND l_shipdate >= '1995-01-01'
  AND l_shipdate <= '1996-12-31'
GROUP BY supp_nation, cust_nation, l_year
ORDER BY supp_nation, cust_nation, l_year;
