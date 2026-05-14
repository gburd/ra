-- MATCH_RECOGNIZE: pattern matching on ordered sequences
SELECT *
FROM orders
MATCH_RECOGNIZE (
    PARTITION BY o_custkey
    ORDER BY o_orderdate
    MEASURES
        FIRST(rising.o_totalprice) AS start_price,
        LAST(rising.o_totalprice) AS end_price,
        COUNT(*) AS streak_length
    ONE ROW PER MATCH
    PATTERN (rising{3,})
    DEFINE rising AS rising.o_totalprice > PREV(rising.o_totalprice)
) mr;
