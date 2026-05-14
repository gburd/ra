-- Nested set operations: complex combination
SELECT p_partkey FROM part WHERE p_size > 40
UNION
(
    SELECT ps_partkey FROM partsupp WHERE ps_supplycost < 100
    INTERSECT
    SELECT l_partkey FROM lineitem WHERE l_quantity > 20
);
