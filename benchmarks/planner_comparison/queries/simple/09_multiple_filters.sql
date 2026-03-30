-- Multiple AND/OR filters
SELECT l_orderkey, l_linenumber
FROM lineitem
WHERE (l_quantity < 10 OR l_discount > 0.05)
  AND l_shipdate >= '1997-01-01';
