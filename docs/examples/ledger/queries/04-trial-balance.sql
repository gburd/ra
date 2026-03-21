-- Trial balance report
-- Shows complex aggregation with multiple joins

-- Generate trial balance for current month
WITH account_balances AS (
    SELECT
        a.account_code,
        a.account_name,
        a.account_type,
        a.normal_balance,
        COALESCE(SUM(
            CASE
                WHEN t.debit_account_code = a.account_code
                THEN t.debit_amount
                ELSE 0
            END
        ), 0) as total_debits,
        COALESCE(SUM(
            CASE
                WHEN t.credit_account_code = a.account_code
                THEN t.credit_amount
                ELSE 0
            END
        ), 0) as total_credits
    FROM chart_of_accounts a
    LEFT JOIN ledger_transactions t
        ON (t.debit_account_code = a.account_code
            OR t.credit_account_code = a.account_code)
        AND t.transaction_date >= DATE_TRUNC('month', CURRENT_DATE)
        AND t.transaction_date < DATE_TRUNC('month', CURRENT_DATE) + INTERVAL '1 month'
    WHERE a.is_leaf = true
    GROUP BY a.account_code, a.account_name, a.account_type, a.normal_balance
),
summarized AS (
    SELECT
        account_code,
        account_name,
        account_type,
        normal_balance,
        total_debits,
        total_credits,
        CASE
            WHEN normal_balance = 'DEBIT'
            THEN total_debits - total_credits
            ELSE total_credits - total_debits
        END as balance
    FROM account_balances
)
SELECT
    account_type,
    account_code,
    account_name,
    total_debits,
    total_credits,
    balance
FROM summarized
WHERE balance != 0
ORDER BY
    CASE account_type
        WHEN 'ASSET' THEN 1
        WHEN 'LIABILITY' THEN 2
        WHEN 'EQUITY' THEN 3
        WHEN 'REVENUE' THEN 4
        WHEN 'EXPENSE' THEN 5
    END,
    account_code;

-- Add summary row
UNION ALL
SELECT
    'TOTAL' as account_type,
    '' as account_code,
    'Trial Balance Total' as account_name,
    SUM(total_debits) as total_debits,
    SUM(total_credits) as total_credits,
    SUM(total_debits) - SUM(total_credits) as balance
FROM summarized;

-- Optimization opportunities:
-- 1. OR-to-UNION transformation for the join condition
-- 2. Aggregate pushdown before join
-- 3. Materialized view for month-end balances
-- 4. Parallel aggregation for large datasets