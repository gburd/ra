-- TPC-DS Query 49
-- Find items with top return ratios across channels
SELECT channel, item, return_ratio, return_rank, currency_rank
FROM (
    SELECT 'web' AS channel, web.item, web.return_ratio,
           web.return_rank, web.currency_rank
    FROM (SELECT ws_item_sk AS item,
                 (CAST(sum(COALESCE(wr_return_quantity, 0)) AS DECIMAL(15,4))
                  / CAST(sum(COALESCE(ws_quantity, 0)) AS DECIMAL(15,4))) AS return_ratio,
                 (CAST(sum(COALESCE(wr_return_amt, 0)) AS DECIMAL(15,4))
                  / CAST(sum(COALESCE(ws_net_paid, 0)) AS DECIMAL(15,4))) AS currency_ratio,
                 rank() OVER (ORDER BY
                     (CAST(sum(COALESCE(wr_return_quantity, 0)) AS DECIMAL(15,4))
                      / CAST(sum(COALESCE(ws_quantity, 0)) AS DECIMAL(15,4)))) AS return_rank,
                 rank() OVER (ORDER BY
                     (CAST(sum(COALESCE(wr_return_amt, 0)) AS DECIMAL(15,4))
                      / CAST(sum(COALESCE(ws_net_paid, 0)) AS DECIMAL(15,4)))) AS currency_rank
          FROM web_sales
          LEFT JOIN web_returns ON wr_order_number = ws_order_number
            AND ws_item_sk = wr_item_sk
          JOIN date_dim ON ws_sold_date_sk = d_date_sk
          WHERE wr_return_amt > 10000
            AND ws_net_profit > 1
            AND ws_net_paid > 0
            AND ws_quantity > 0
            AND d_year = 2001
            AND d_moy = 12
          GROUP BY ws_item_sk) web
    WHERE web.return_rank <= 10 OR web.currency_rank <= 10
    UNION
    SELECT 'catalog' AS channel, catalog.item, catalog.return_ratio,
           catalog.return_rank, catalog.currency_rank
    FROM (SELECT cs_item_sk AS item,
                 (CAST(sum(COALESCE(cr_return_quantity, 0)) AS DECIMAL(15,4))
                  / CAST(sum(COALESCE(cs_quantity, 0)) AS DECIMAL(15,4))) AS return_ratio,
                 (CAST(sum(COALESCE(cr_return_amount, 0)) AS DECIMAL(15,4))
                  / CAST(sum(COALESCE(cs_net_paid, 0)) AS DECIMAL(15,4))) AS currency_ratio,
                 rank() OVER (ORDER BY
                     (CAST(sum(COALESCE(cr_return_quantity, 0)) AS DECIMAL(15,4))
                      / CAST(sum(COALESCE(cs_quantity, 0)) AS DECIMAL(15,4)))) AS return_rank,
                 rank() OVER (ORDER BY
                     (CAST(sum(COALESCE(cr_return_amount, 0)) AS DECIMAL(15,4))
                      / CAST(sum(COALESCE(cs_net_paid, 0)) AS DECIMAL(15,4)))) AS currency_rank
          FROM catalog_sales
          LEFT JOIN catalog_returns ON cr_order_number = cs_order_number
            AND cs_item_sk = cr_item_sk
          JOIN date_dim ON cs_sold_date_sk = d_date_sk
          WHERE cr_return_amount > 10000
            AND cs_net_profit > 1
            AND cs_net_paid > 0
            AND cs_quantity > 0
            AND d_year = 2001
            AND d_moy = 12
          GROUP BY cs_item_sk) catalog
    WHERE catalog.return_rank <= 10 OR catalog.currency_rank <= 10
    UNION
    SELECT 'store' AS channel, store.item, store.return_ratio,
           store.return_rank, store.currency_rank
    FROM (SELECT ss_item_sk AS item,
                 (CAST(sum(COALESCE(sr_return_quantity, 0)) AS DECIMAL(15,4))
                  / CAST(sum(COALESCE(ss_quantity, 0)) AS DECIMAL(15,4))) AS return_ratio,
                 (CAST(sum(COALESCE(sr_return_amt, 0)) AS DECIMAL(15,4))
                  / CAST(sum(COALESCE(ss_net_paid, 0)) AS DECIMAL(15,4))) AS currency_ratio,
                 rank() OVER (ORDER BY
                     (CAST(sum(COALESCE(sr_return_quantity, 0)) AS DECIMAL(15,4))
                      / CAST(sum(COALESCE(ss_quantity, 0)) AS DECIMAL(15,4)))) AS return_rank,
                 rank() OVER (ORDER BY
                     (CAST(sum(COALESCE(sr_return_amt, 0)) AS DECIMAL(15,4))
                      / CAST(sum(COALESCE(ss_net_paid, 0)) AS DECIMAL(15,4)))) AS currency_rank
          FROM store_sales
          LEFT JOIN store_returns ON sr_ticket_number = ss_ticket_number
            AND ss_item_sk = sr_item_sk
          JOIN date_dim ON ss_sold_date_sk = d_date_sk
          WHERE sr_return_amt > 10000
            AND ss_net_profit > 1
            AND ss_net_paid > 0
            AND ss_quantity > 0
            AND d_year = 2001
            AND d_moy = 12
          GROUP BY ss_item_sk) store
    WHERE store.return_rank <= 10 OR store.currency_rank <= 10
) sub
ORDER BY 1, 4, 5, 2
LIMIT 100;
