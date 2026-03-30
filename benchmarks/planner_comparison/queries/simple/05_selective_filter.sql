-- Highly selective filter (1%)
SELECT * FROM lineitem
WHERE l_quantity < 2
  AND l_discount > 0.09
  AND l_discount < 0.11;
