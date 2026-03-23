SELECT SUM(l_extendedprice * (1 - l_discount)) AS revenue
FROM lineitem, part
WHERE l_partkey = p_partkey
  AND (
    (p_brand = 'Brand#12' AND l_quantity <= 11)
    OR (p_brand = 'Brand#23' AND l_quantity <= 20)
  );
