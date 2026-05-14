-- UNION: combine customer and supplier names by nation
SELECT c_name AS entity_name, 'customer' AS entity_type, c_nationkey AS nationkey
FROM customer
WHERE c_acctbal > 9000
UNION
SELECT s_name AS entity_name, 'supplier' AS entity_type, s_nationkey AS nationkey
FROM supplier
WHERE s_acctbal > 9000;
