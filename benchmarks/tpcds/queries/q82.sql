-- TPC-DS Query 82: Report inventory for items in specific price ranges and dates
-- Find items with specific price ranges that had inventory in certain date range
SELECT
    i_item_id,
    i_item_desc,
    i_current_price
FROM item
JOIN inventory ON inv_item_sk = i_item_sk
JOIN date_dim ON inv_date_sk = d_date_sk
JOIN store_sales ON ss_item_sk = i_item_sk
WHERE i_current_price BETWEEN 62 AND 92
    AND d_date BETWEEN '2000-05-25' AND (DATE '2000-05-25' + INTERVAL '60 days')
    AND inv_quantity_on_hand BETWEEN 100 AND 500
GROUP BY i_item_id, i_item_desc, i_current_price
ORDER BY i_item_id
LIMIT 100;
