-- Basic account balance query
-- Shows fundamental optimization: filter pushdown and index usage

-- Get current balance for cash account
SELECT
    a.account_code,
    a.account_name,
    COALESCE(SUM(
        CASE
            WHEN t.debit_account_code = a.account_code
            THEN t.debit_amount
            WHEN t.credit_account_code = a.account_code
            THEN -t.credit_amount
            ELSE 0
        END
    ), 0) as current_balance
FROM chart_of_accounts a
LEFT JOIN ledger_transactions t
    ON (t.debit_account_code = a.account_code
        OR t.credit_account_code = a.account_code)
WHERE a.account_code = '1010'  -- Cash account
GROUP BY a.account_code, a.account_name;

-- Optimization opportunities:
-- 1. Use index on account_code
-- 2. Push filter before join
-- 3. Consider covering index for transaction amounts