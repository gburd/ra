-- EXCEPT ALL: supplier nations not in customer nations
SELECT s_nationkey AS nationkey
FROM supplier
EXCEPT ALL
SELECT c_nationkey AS nationkey
FROM customer;
