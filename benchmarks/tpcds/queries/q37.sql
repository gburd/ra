-- TPC-DS Query 37
-- Find specific items with inventory above a threshold, within current price range
SELECT i_item_id,
       i_item_desc,
       i_current_price
FROM item
JOIN inventory ON inv_item_sk = i_item_sk
JOIN date_dim ON d_date_sk = inv_date_sk
JOIN catalog_sales ON cs_item_sk = i_item_sk
WHERE i_current_price BETWEEN 68 AND 68 + 30
  AND inv_quantity_on_hand BETWEEN 100 AND 500
  AND d_date BETWEEN '2000-02-01' AND (CAST('2000-02-01' AS DATE) + INTERVAL '60 days')
GROUP BY i_item_id, i_item_desc, i_current_price
ORDER BY i_item_id
LIMIT 100;
