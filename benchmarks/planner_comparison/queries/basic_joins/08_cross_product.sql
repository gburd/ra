-- Small cross product (nation x region)
SELECT n.n_name, r.r_name
FROM nation n, region r
WHERE n.n_regionkey = r.r_regionkey;
