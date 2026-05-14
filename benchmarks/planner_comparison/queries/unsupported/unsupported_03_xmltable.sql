-- XMLTABLE: parse XML data
SELECT x.*
FROM orders o,
XMLTABLE('/order/items/item'
    PASSING o.o_comment::xml
    COLUMNS
        item_name TEXT PATH 'name',
        item_qty INTEGER PATH 'quantity',
        item_price NUMERIC PATH 'price'
) AS x
WHERE o.o_orderkey < 100;
