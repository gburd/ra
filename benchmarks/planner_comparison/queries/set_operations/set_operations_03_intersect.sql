-- INTERSECT: nations that have both customers and suppliers
SELECT c_nationkey AS nationkey
FROM customer
INTERSECT
SELECT s_nationkey AS nationkey
FROM supplier;
