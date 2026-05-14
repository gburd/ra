-- Semi-join via EXISTS: suppliers with parts in stock
SELECT s.s_suppkey, s.s_name, s.s_phone
FROM supplier s
WHERE EXISTS (
    SELECT 1 FROM partsupp ps
    WHERE ps.ps_suppkey = s.s_suppkey
      AND ps.ps_availqty > 1000
);
