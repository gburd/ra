-- TPC-DS Query 44
-- Find store items with above-average net profit in a specific month
SELECT asceding.rnk, i1.i_product_name AS best_performing,
       desceding.rnk, i2.i_product_name AS worst_performing
FROM (SELECT item_sk, rank() OVER (ORDER BY rank_col ASC) AS rnk
      FROM (SELECT ss_item_sk AS item_sk,
                   avg(ss_net_profit) AS rank_col
            FROM store_sales ss1
            WHERE ss_store_sk = 4
            GROUP BY ss_item_sk
            HAVING avg(ss_net_profit) > 0.9 *
                (SELECT avg(ss_net_profit) AS rank_col
                 FROM store_sales
                 WHERE ss_store_sk = 4
                   AND ss_addr_sk IS NULL
                 GROUP BY ss_store_sk)) v1) asceding
JOIN (SELECT item_sk, rank() OVER (ORDER BY rank_col DESC) AS rnk
      FROM (SELECT ss_item_sk AS item_sk,
                   avg(ss_net_profit) AS rank_col
            FROM store_sales ss1
            WHERE ss_store_sk = 4
            GROUP BY ss_item_sk
            HAVING avg(ss_net_profit) > 0.9 *
                (SELECT avg(ss_net_profit) AS rank_col
                 FROM store_sales
                 WHERE ss_store_sk = 4
                   AND ss_addr_sk IS NULL
                 GROUP BY ss_store_sk)) v2) desceding
  ON asceding.rnk = desceding.rnk
JOIN item i1 ON asceding.item_sk = i1.i_item_sk
JOIN item i2 ON desceding.item_sk = i2.i_item_sk
WHERE asceding.rnk <= 10
ORDER BY asceding.rnk
LIMIT 100;
