-- Running balance calculation
-- Shows window function optimization

-- Calculate running balance for a specific account
WITH account_movements AS (
    SELECT
        transaction_date,
        id,
        'DEBIT' as entry_type,
        debit_amount as amount
    FROM ledger_transactions
    WHERE debit_account_code = '1010'

    UNION ALL

    SELECT
        transaction_date,
        id,
        'CREDIT' as entry_type,
        -credit_amount as amount
    FROM ledger_transactions
    WHERE credit_account_code = '1010'
)
SELECT
    transaction_date,
    id,
    entry_type,
    amount,
    SUM(amount) OVER (
        ORDER BY transaction_date, id
        ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW
    ) as running_balance,
    AVG(ABS(amount)) OVER (
        ORDER BY transaction_date, id
        ROWS BETWEEN 30 PRECEDING AND CURRENT ROW
    ) as moving_avg_30_transactions
FROM account_movements
ORDER BY transaction_date, id;

-- Optimization opportunities:
-- 1. Index on (account_code, transaction_date) for sorted access
-- 2. Covering index to avoid table lookups
-- 3. Partition by date for very large datasets