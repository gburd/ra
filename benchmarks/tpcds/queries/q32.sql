-- TPC-DS Query 32
-- Compute the excess discount amount for items in specific manufacturer ranges
SELECT sum(cs_ext_discount_amt) AS excess_discount_amount
FROM catalog_sales
JOIN item ON i_item_sk = cs_item_sk
JOIN date_dim ON d_date_sk = cs_sold_date_sk
WHERE i_manufact_id = 977
  AND d_date BETWEEN '2000-01-27' AND (CAST('2000-01-27' AS DATE) + INTERVAL '90 days')
  AND cs_ext_discount_amt > (
      SELECT 1.3 * avg(cs_ext_discount_amt)
      FROM catalog_sales
      JOIN date_dim ON d_date_sk = cs_sold_date_sk
      WHERE cs_item_sk = i_item_sk
        AND d_date BETWEEN '2000-01-27' AND (CAST('2000-01-27' AS DATE) + INTERVAL '90 days')
  )
LIMIT 100;
