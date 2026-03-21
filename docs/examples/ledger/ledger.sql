-- Enable UUID extension for primary keys
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- Drop the table
DROP TABLE IF EXISTS currencies CASCADE;

-- Currency master table
CREATE TABLE currencies (
    currency_code CHAR(3) PRIMARY KEY,
    currency_name VARCHAR(100) NOT NULL,
    decimal_places SMALLINT NOT NULL DEFAULT 2,
    is_active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Drop the table
DROP TABLE IF EXISTS exchange_rates CASCADE;

-- Exchange rates table for currency conversion
CREATE TABLE exchange_rates (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    from_currency CHAR(3) NOT NULL REFERENCES currencies(currency_code),
    to_currency CHAR(3) NOT NULL REFERENCES currencies(currency_code),
    rate DECIMAL(20,10) NOT NULL,
    effective_date DATE NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(from_currency, to_currency, effective_date)
);

-- Drop the table
DROP TABLE IF EXISTS chart_of_accounts CASCADE;

-- Chart of accounts with hierarchy support
CREATE TABLE chart_of_accounts (
    account_code VARCHAR(20) PRIMARY KEY,
    parent_account_code VARCHAR(20) REFERENCES chart_of_accounts(account_code),
    account_name VARCHAR(255) NOT NULL,
    account_type VARCHAR(20) NOT NULL CHECK (account_type IN ('ASSET', 'LIABILITY', 'EQUITY', 'REVENUE', 'EXPENSE')),
    normal_balance VARCHAR(6) NOT NULL CHECK (normal_balance IN ('DEBIT', 'CREDIT')),
    is_active BOOLEAN NOT NULL DEFAULT true,
    is_leaf BOOLEAN NOT NULL DEFAULT true, -- Can this account have transactions posted to it
    level INTEGER NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Drop the table
DROP TABLE IF EXISTS journal_entries CASCADE;

-- Journal entries header
CREATE TABLE journal_entries (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    entry_number VARCHAR(50) NOT NULL UNIQUE,
    entry_date DATE NOT NULL,
    description TEXT,
    reference VARCHAR(100),
    created_by VARCHAR(100),
    posted_at TIMESTAMPTZ,
    is_posted BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Drop the table
DROP TABLE IF EXISTS ledger_transactions CASCADE;

-- Single-row transaction model with multi-currency support
CREATE TABLE ledger_transactions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    journal_entry_id UUID NOT NULL REFERENCES journal_entries(id) ON DELETE CASCADE,
    transaction_date DATE NOT NULL,

    -- Debit side
    debit_account_code VARCHAR(20) NOT NULL REFERENCES chart_of_accounts(account_code),
    debit_amount DECIMAL(20,4) NOT NULL CHECK (debit_amount > 0),
    debit_currency CHAR(3) NOT NULL REFERENCES currencies(currency_code),

    -- Credit side
    credit_account_code VARCHAR(20) NOT NULL REFERENCES chart_of_accounts(account_code),
    credit_amount DECIMAL(20,4) NOT NULL CHECK (credit_amount > 0),
    credit_currency CHAR(3) NOT NULL REFERENCES currencies(currency_code),

    -- Base currency amounts for reporting
    base_currency CHAR(3) NOT NULL REFERENCES currencies(currency_code),
    base_debit_amount DECIMAL(20,4) NOT NULL,
    base_credit_amount DECIMAL(20,4) NOT NULL,

    -- Exchange rates used
    debit_exchange_rate DECIMAL(20,10) NOT NULL DEFAULT 1.0,
    credit_exchange_rate DECIMAL(20,10) NOT NULL DEFAULT 1.0,

    description TEXT,
    reference VARCHAR(100),

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Ensure debit != credit account
    CHECK (debit_account_code != credit_account_code)
);

-- Indexes for performance
CREATE INDEX idx_ledger_transactions_journal_entry ON ledger_transactions(journal_entry_id);
CREATE INDEX idx_ledger_transactions_date ON ledger_transactions(transaction_date);
CREATE INDEX idx_ledger_transactions_debit_account ON ledger_transactions(debit_account_code);
CREATE INDEX idx_ledger_transactions_credit_account ON ledger_transactions(credit_account_code);
CREATE INDEX idx_ledger_transactions_base_currency ON ledger_transactions(base_currency);
CREATE INDEX idx_exchange_rates_lookup ON exchange_rates(from_currency, to_currency, effective_date);

-- Drop the view
DROP MATERIALIZED VIEW IF EXISTS account_balances CASCADE;

-- Account balances materialized view for performance
CREATE MATERIALIZED VIEW IF NOT EXISTS account_balances AS
WITH debit_balances AS (
    SELECT
	debit_account_code as account_code,
	debit_currency as currency_code,
	SUM(debit_amount) as debit_total,
	0 as credit_total,
	SUM(base_debit_amount) as base_debit_total,
	0 as base_credit_total
    FROM ledger_transactions lt
    JOIN journal_entries je ON lt.journal_entry_id = je.id
    WHERE je.is_posted = true
    GROUP BY debit_account_code, debit_currency
),
credit_balances AS (
    SELECT
	credit_account_code as account_code,
	credit_currency as currency_code,
	0 as debit_total,
	SUM(credit_amount) as credit_total,
	0 as base_debit_total,
	SUM(base_credit_amount) as base_credit_total
    FROM ledger_transactions lt
    JOIN journal_entries je ON lt.journal_entry_id = je.id
    WHERE je.is_posted = true
    GROUP BY credit_account_code, credit_currency
),
combined_balances AS (
    SELECT * FROM debit_balances
    UNION ALL
    SELECT * FROM credit_balances
)
SELECT
    account_code,
    currency_code,
    SUM(debit_total) as total_debits,
    SUM(credit_total) as total_credits,
    SUM(debit_total) - SUM(credit_total) as balance,
    SUM(base_debit_total) as base_total_debits,
    SUM(base_credit_total) as base_total_credits,
    SUM(base_debit_total) - SUM(base_credit_total) as base_balance
FROM combined_balances
GROUP BY account_code, currency_code;

CREATE UNIQUE INDEX idx_account_balances_unique ON account_balances(account_code, currency_code);

-- Function to get exchange rate
CREATE OR REPLACE FUNCTION get_exchange_rate(
    p_from_currency CHAR(3),
    p_to_currency CHAR(3),
    p_date DATE
) RETURNS DECIMAL(20,10) AS $$
DECLARE
    v_rate DECIMAL(20,10);
BEGIN
    -- Same currency
    IF p_from_currency = p_to_currency THEN
	RETURN 1.0;
    END IF;

    -- Get most recent rate on or before the date
    SELECT rate INTO v_rate
    FROM exchange_rates
    WHERE from_currency = p_from_currency
      AND to_currency = p_to_currency
      AND effective_date <= p_date
    ORDER BY effective_date DESC
    LIMIT 1;

    IF v_rate IS NULL THEN
	RAISE EXCEPTION 'No exchange rate found for % to % on %', p_from_currency, p_to_currency, p_date;
    END IF;

    RETURN v_rate;
END;
$$ LANGUAGE plpgsql;

-- Function to create a transaction with automatic currency conversion
CREATE OR REPLACE FUNCTION create_ledger_transaction(
    p_journal_entry_id UUID,
    p_transaction_date DATE,
    p_debit_account VARCHAR(20),
    p_debit_amount DECIMAL(20,4),
    p_debit_currency CHAR(3),
    p_credit_account VARCHAR(20),
    p_credit_amount DECIMAL(20,4),
    p_credit_currency CHAR(3),
    p_base_currency CHAR(3),
    p_description TEXT DEFAULT NULL,
    p_reference VARCHAR(100) DEFAULT NULL
) RETURNS UUID AS $$
DECLARE
    v_transaction_id UUID;
    v_debit_rate DECIMAL(20,10);
    v_credit_rate DECIMAL(20,10);
    v_base_debit_amount DECIMAL(20,4);
    v_base_credit_amount DECIMAL(20,4);
BEGIN
    -- Get exchange rates
    v_debit_rate := get_exchange_rate(p_debit_currency, p_base_currency, p_transaction_date);
    v_credit_rate := get_exchange_rate(p_credit_currency, p_base_currency, p_transaction_date);

    -- Calculate base amounts
    v_base_debit_amount := p_debit_amount * v_debit_rate;
    v_base_credit_amount := p_credit_amount * v_credit_rate;

    -- Insert transaction
    INSERT INTO ledger_transactions (
	journal_entry_id, transaction_date,
	debit_account_code, debit_amount, debit_currency,
	credit_account_code, credit_amount, credit_currency,
	base_currency, base_debit_amount, base_credit_amount,
	debit_exchange_rate, credit_exchange_rate,
	description, reference
    ) VALUES (
	p_journal_entry_id, p_transaction_date,
	p_debit_account, p_debit_amount, p_debit_currency,
	p_credit_account, p_credit_amount, p_credit_currency,
	p_base_currency, v_base_debit_amount, v_base_credit_amount,
	v_debit_rate, v_credit_rate,
	p_description, p_reference
    ) RETURNING id INTO v_transaction_id;

    RETURN v_transaction_id;
END;
$$ LANGUAGE plpgsql;

-- Trigger to refresh materialized view
CREATE OR REPLACE FUNCTION refresh_account_balances()
RETURNS TRIGGER AS $$
BEGIN
    REFRESH MATERIALIZED VIEW CONCURRENTLY account_balances;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_refresh_balances
    AFTER INSERT OR UPDATE OR DELETE ON ledger_transactions
    FOR EACH STATEMENT
    EXECUTE FUNCTION refresh_account_balances();

-- Sample data
INSERT INTO currencies (currency_code, currency_name) VALUES
('USD', 'US Dollar'),
('EUR', 'Euro'),
('GBP', 'British Pound');

INSERT INTO exchange_rates (from_currency, to_currency, rate, effective_date) VALUES
('EUR', 'USD', 1.1000, '2025-01-01'),
('GBP', 'USD', 1.2500, '2025-01-01'),
('USD', 'EUR', 0.9091, '2025-01-01'),
('USD', 'GBP', 0.8000, '2025-01-01');

INSERT INTO chart_of_accounts (account_code, account_name, account_type, normal_balance) VALUES
('1000', 'Cash', 'ASSET', 'DEBIT'),
('1100', 'Accounts Receivable', 'ASSET', 'DEBIT'),
('2000', 'Accounts Payable', 'LIABILITY', 'CREDIT'),
('3000', 'Equity', 'EQUITY', 'CREDIT'),
('4000', 'Revenue', 'REVENUE', 'CREDIT'),
('5000', 'Expenses', 'EXPENSE', 'DEBIT');

-- ============================================================================
-- STORED PROCEDURES FOR DOUBLE-ENTRY ACCOUNTING OPERATIONS
-- ============================================================================

-- 1. JOURNAL ENTRY MANAGEMENT
-- ============================================================================

-- Create a new journal entry
CREATE OR REPLACE FUNCTION create_journal_entry(
    p_entry_number VARCHAR(50),
    p_entry_date DATE,
    p_description TEXT DEFAULT NULL,
    p_reference VARCHAR(100) DEFAULT NULL,
    p_created_by VARCHAR(100) DEFAULT NULL
) RETURNS UUID AS $$
DECLARE
    v_entry_id UUID;
BEGIN
    INSERT INTO journal_entries (
	entry_number, entry_date, description, reference, created_by
    ) VALUES (
	p_entry_number, p_entry_date, p_description, p_reference, p_created_by
    ) RETURNING id INTO v_entry_id;

    RETURN v_entry_id;
END;
$$ LANGUAGE plpgsql;

-- Post a journal entry (mark as posted)
CREATE OR REPLACE FUNCTION post_journal_entry(
    p_entry_id UUID
) RETURNS BOOLEAN AS $$
DECLARE
    v_debit_total DECIMAL(20,4);
    v_credit_total DECIMAL(20,4);
BEGIN
    -- Verify entry exists and is not already posted
    IF NOT EXISTS (SELECT 1 FROM journal_entries WHERE id = p_entry_id AND is_posted = false) THEN
	RAISE EXCEPTION 'Journal entry not found or already posted';
    END IF;

    -- Verify entry is balanced in base currency
    SELECT
	SUM(base_debit_amount),
	SUM(base_credit_amount)
    INTO v_debit_total, v_credit_total
    FROM ledger_transactions
    WHERE journal_entry_id = p_entry_id;

    IF ABS(v_debit_total - v_credit_total) > 0.01 THEN
	RAISE EXCEPTION 'Journal entry is not balanced. Debit: %, Credit: %', v_debit_total, v_credit_total;
    END IF;

    -- Post the entry
    UPDATE journal_entries
    SET is_posted = true, posted_at = NOW()
    WHERE id = p_entry_id;

    RETURN true;
END;
$$ LANGUAGE plpgsql;

-- Reverse a journal entry
CREATE OR REPLACE FUNCTION reverse_journal_entry(
    p_entry_id UUID,
    p_reversal_date DATE,
    p_reversal_reason TEXT
) RETURNS UUID AS $$
DECLARE
    v_original_entry journal_entries%ROWTYPE;
    v_reversal_id UUID;
    v_transaction RECORD;
BEGIN
    -- Get original entry
    SELECT * INTO v_original_entry
    FROM journal_entries
    WHERE id = p_entry_id AND is_posted = true;

    IF NOT FOUND THEN
	RAISE EXCEPTION 'Original journal entry not found or not posted';
    END IF;

    -- Create reversal entry
    v_reversal_id := create_journal_entry(
	v_original_entry.entry_number || '-REV',
	p_reversal_date,
	'REVERSAL: ' || COALESCE(p_reversal_reason, v_original_entry.description),
	'REV-' || v_original_entry.reference,
	'SYSTEM'
    );

    -- Create reverse transactions
    FOR v_transaction IN
	SELECT * FROM ledger_transactions WHERE journal_entry_id = p_entry_id
    LOOP
	PERFORM create_ledger_transaction(
	    v_reversal_id,
	    p_reversal_date,
	    v_transaction.credit_account_code, -- Swap debit/credit
	    v_transaction.credit_amount,
	    v_transaction.credit_currency,
	    v_transaction.debit_account_code,
	    v_transaction.debit_amount,
	    v_transaction.debit_currency,
	    v_transaction.base_currency,
	    'REVERSAL: ' || COALESCE(v_transaction.description, ''),
	    v_transaction.reference
	);
    END LOOP;

    -- Auto-post reversal
    PERFORM post_journal_entry(v_reversal_id);

    RETURN v_reversal_id;
END;
$$ LANGUAGE plpgsql;

-- 2. ACCOUNT MANAGEMENT
-- ============================================================================

-- Create account
CREATE OR REPLACE FUNCTION create_account(
    p_account_code VARCHAR(20),
    p_account_name VARCHAR(255),
    p_account_type VARCHAR(20),
    p_normal_balance VARCHAR(6),
    p_parent_account VARCHAR(20) DEFAULT NULL
) RETURNS BOOLEAN AS $$
DECLARE
    v_level INTEGER := 1;
BEGIN
    -- Calculate level if parent exists
    IF p_parent_account IS NOT NULL THEN
	SELECT level + 1 INTO v_level
	FROM chart_of_accounts
	WHERE account_code = p_parent_account;

	IF NOT FOUND THEN
	    RAISE EXCEPTION 'Parent account % not found', p_parent_account;
	END IF;

	-- Update parent to not be leaf
	UPDATE chart_of_accounts
	SET is_leaf = false
	WHERE account_code = p_parent_account;
    END IF;

    INSERT INTO chart_of_accounts (
	account_code, account_name, account_type, normal_balance,
	parent_account_code, level
    ) VALUES (
	p_account_code, p_account_name, p_account_type, p_normal_balance,
	p_parent_account, v_level
    );

    RETURN true;
END;
$$ LANGUAGE plpgsql;

-- Get account balance
CREATE OR REPLACE FUNCTION get_account_balance(
    p_account_code VARCHAR(20),
    p_currency_code CHAR(3) DEFAULT NULL,
    p_as_of_date DATE DEFAULT CURRENT_DATE
) RETURNS TABLE(
    account_code VARCHAR(20),
    currency_code CHAR(3),
    balance DECIMAL(20,4),
    base_balance DECIMAL(20,4)
) AS $$
BEGIN
    RETURN QUERY
    WITH account_transactions AS (
	SELECT
	    CASE
		WHEN lt.debit_account_code = p_account_code THEN lt.debit_currency
		ELSE lt.credit_currency
	    END as currency,
	    CASE
		WHEN lt.debit_account_code = p_account_code THEN lt.debit_amount
		ELSE -lt.credit_amount
	    END as amount,
	    CASE
		WHEN lt.debit_account_code = p_account_code THEN lt.base_debit_amount
		ELSE -lt.base_credit_amount
	    END as base_amount
	FROM ledger_transactions lt
	JOIN journal_entries je ON lt.journal_entry_id = je.id
	WHERE (lt.debit_account_code = p_account_code OR lt.credit_account_code = p_account_code)
	  AND je.is_posted = true
	  AND lt.transaction_date <= p_as_of_date
	  AND (p_currency_code IS NULL OR
	       (lt.debit_account_code = p_account_code AND lt.debit_currency = p_currency_code) OR
	       (lt.credit_account_code = p_account_code AND lt.credit_currency = p_currency_code))
    )
    SELECT
	p_account_code,
	at.currency,
	SUM(at.amount),
	SUM(at.base_amount)
    FROM account_transactions at
    GROUP BY at.currency;
END;
$$ LANGUAGE plpgsql;

-- 3. TRIAL BALANCE AND REPORTING
-- ============================================================================

-- Generate trial balance
CREATE OR REPLACE FUNCTION get_trial_balance(
    p_as_of_date DATE DEFAULT CURRENT_DATE,
    p_base_currency CHAR(3) DEFAULT 'USD'
) RETURNS TABLE(
    account_code VARCHAR(20),
    account_name VARCHAR(255),
    account_type VARCHAR(20),
    debit_balance DECIMAL(20,4),
    credit_balance DECIMAL(20,4)
) AS $$
BEGIN
    RETURN QUERY
    WITH account_balances AS (
	SELECT
	    coa.account_code,
	    coa.account_name,
	    coa.account_type,
	    coa.normal_balance,
	    COALESCE(SUM(
		CASE WHEN lt.debit_account_code = coa.account_code
		     THEN lt.base_debit_amount ELSE 0 END
	    ), 0) as total_debits,
	    COALESCE(SUM(
		CASE WHEN lt.credit_account_code = coa.account_code
		     THEN lt.base_credit_amount ELSE 0 END
	    ), 0) as total_credits
	FROM chart_of_accounts coa
	LEFT JOIN ledger_transactions lt ON (
	    lt.debit_account_code = coa.account_code OR
	    lt.credit_account_code = coa.account_code
	)
	LEFT JOIN journal_entries je ON lt.journal_entry_id = je.id
	WHERE coa.is_active = true
	  AND coa.is_leaf = true
	  AND (je.is_posted = true OR je.id IS NULL)
	  AND (lt.transaction_date <= p_as_of_date OR lt.transaction_date IS NULL)
	  AND (lt.base_currency = p_base_currency OR lt.base_currency IS NULL)
	GROUP BY coa.account_code, coa.account_name, coa.account_type, coa.normal_balance
    )
    SELECT
	ab.account_code,
	ab.account_name,
	ab.account_type,
	CASE WHEN ab.total_debits - ab.total_credits > 0
	     THEN ab.total_debits - ab.total_credits ELSE 0 END,
	CASE WHEN ab.total_credits - ab.total_debits > 0
	     THEN ab.total_credits - ab.total_debits ELSE 0 END
    FROM account_balances ab
    WHERE ab.total_debits != 0 OR ab.total_credits != 0
    ORDER BY ab.account_code;
END;
$$ LANGUAGE plpgsql;

-- Generate balance sheet
CREATE OR REPLACE FUNCTION get_balance_sheet(
    p_as_of_date DATE DEFAULT CURRENT_DATE,
    p_base_currency CHAR(3) DEFAULT 'USD'
) RETURNS TABLE(
    section VARCHAR(20),
    account_code VARCHAR(20),
    account_name VARCHAR(255),
    balance DECIMAL(20,4)
) AS $$
BEGIN
    RETURN QUERY
    SELECT
	tb.account_type as section,
	tb.account_code,
	tb.account_name,
	CASE
	    WHEN tb.account_type IN ('ASSET', 'EXPENSE') THEN tb.debit_balance - tb.credit_balance
	    ELSE tb.credit_balance - tb.debit_balance
	END as balance
    FROM get_trial_balance(p_as_of_date, p_base_currency) tb
    WHERE tb.account_type IN ('ASSET', 'LIABILITY', 'EQUITY')
    ORDER BY tb.account_type, tb.account_code;
END;
$$ LANGUAGE plpgsql;

-- Generate income statement
CREATE OR REPLACE FUNCTION get_income_statement(
    p_start_date DATE,
    p_end_date DATE,
    p_base_currency CHAR(3) DEFAULT 'USD'
) RETURNS TABLE(
    section VARCHAR(20),
    account_code VARCHAR(20),
    account_name VARCHAR(255),
    amount DECIMAL(20,4)
) AS $$
BEGIN
    RETURN QUERY
    WITH period_balances AS (
	SELECT
	    coa.account_code,
	    coa.account_name,
	    coa.account_type,
	    COALESCE(SUM(
		CASE WHEN lt.debit_account_code = coa.account_code
		     THEN lt.base_debit_amount ELSE 0 END
	    ), 0) as total_debits,
	    COALESCE(SUM(
		CASE WHEN lt.credit_account_code = coa.account_code
		     THEN lt.base_credit_amount ELSE 0 END
	    ), 0) as total_credits
	FROM chart_of_accounts coa
	LEFT JOIN ledger_transactions lt ON (
	    lt.debit_account_code = coa.account_code OR
	    lt.credit_account_code = coa.account_code
	)
	LEFT JOIN journal_entries je ON lt.journal_entry_id = je.id
	WHERE coa.is_active = true
	  AND coa.is_leaf = true
	  AND coa.account_type IN ('REVENUE', 'EXPENSE')
	  AND (je.is_posted = true OR je.id IS NULL)
	  AND (lt.transaction_date BETWEEN p_start_date AND p_end_date OR lt.transaction_date IS NULL)
	  AND (lt.base_currency = p_base_currency OR lt.base_currency IS NULL)
	GROUP BY coa.account_code, coa.account_name, coa.account_type
    )
    SELECT
	pb.account_type as section,
	pb.account_code,
	pb.account_name,
	CASE
	    WHEN pb.account_type = 'REVENUE' THEN pb.total_credits - pb.total_debits
	    ELSE pb.total_debits - pb.total_credits
	END as amount
    FROM period_balances pb
    WHERE pb.total_debits != 0 OR pb.total_credits != 0
    ORDER BY pb.account_type DESC, pb.account_code;
END;
$$ LANGUAGE plpgsql;

-- 4. CURRENCY OPERATIONS
-- ============================================================================

-- Add or update exchange rate
CREATE OR REPLACE FUNCTION set_exchange_rate(
    p_from_currency CHAR(3),
    p_to_currency CHAR(3),
    p_rate DECIMAL(20,10),
    p_effective_date DATE DEFAULT CURRENT_DATE
) RETURNS BOOLEAN AS $$
BEGIN
    INSERT INTO exchange_rates (from_currency, to_currency, rate, effective_date)
    VALUES (p_from_currency, p_to_currency, p_rate, p_effective_date)
    ON CONFLICT (from_currency, to_currency, effective_date)
    DO UPDATE SET rate = p_rate;

    RETURN true;
END;
$$ LANGUAGE plpgsql;

-- Convert amount between currencies
CREATE OR REPLACE FUNCTION convert_currency(
    p_amount DECIMAL(20,4),
    p_from_currency CHAR(3),
    p_to_currency CHAR(3),
    p_date DATE DEFAULT CURRENT_DATE
) RETURNS DECIMAL(20,4) AS $$
DECLARE
    v_rate DECIMAL(20,10);
BEGIN
    v_rate := get_exchange_rate(p_from_currency, p_to_currency, p_date);
    RETURN p_amount * v_rate;
END;
$$ LANGUAGE plpgsql;

-- 5. AUDIT AND VALIDATION
-- ============================================================================

-- Validate all posted entries are balanced
CREATE OR REPLACE FUNCTION validate_all_entries()
RETURNS TABLE(
    entry_id UUID,
    entry_number VARCHAR(50),
    debit_total DECIMAL(20,4),
    credit_total DECIMAL(20,4),
    difference DECIMAL(20,4)
) AS $$
BEGIN
    RETURN QUERY
    SELECT
	je.id,
	je.entry_number,
	SUM(lt.base_debit_amount) as debit_total,
	SUM(lt.base_credit_amount) as credit_total,
	SUM(lt.base_debit_amount) - SUM(lt.base_credit_amount) as difference
    FROM journal_entries je
    JOIN ledger_transactions lt ON je.id = lt.journal_entry_id
    WHERE je.is_posted = true
    GROUP BY je.id, je.entry_number
    HAVING ABS(SUM(lt.base_debit_amount) - SUM(lt.base_credit_amount)) > 0.01
    ORDER BY je.entry_number;
END;
$$ LANGUAGE plpgsql;

-- Get account activity
CREATE OR REPLACE FUNCTION get_account_activity(
    p_account_code VARCHAR(20),
    p_start_date DATE DEFAULT NULL,
    p_end_date DATE DEFAULT NULL,
    p_limit INTEGER DEFAULT 100
) RETURNS TABLE(
    transaction_date DATE,
    entry_number VARCHAR(50),
    description TEXT,
    debit_amount DECIMAL(20,4),
    credit_amount DECIMAL(20,4),
    currency CHAR(3),
    running_balance DECIMAL(20,4)
) AS $$
BEGIN
    RETURN QUERY
    WITH account_movements AS (
	SELECT
	    lt.transaction_date,
	    je.entry_number,
	    COALESCE(lt.description, je.description) as description,
	    CASE WHEN lt.debit_account_code = p_account_code
		 THEN lt.debit_amount ELSE 0 END as debit_amount,
	    CASE WHEN lt.credit_account_code = p_account_code
		 THEN lt.credit_amount ELSE 0 END as credit_amount,
	    CASE WHEN lt.debit_account_code = p_account_code
		 THEN lt.debit_currency ELSE lt.credit_currency END as currency,
	    CASE WHEN lt.debit_account_code = p_account_code
		 THEN lt.base_debit_amount ELSE -lt.base_credit_amount END as net_amount
	FROM ledger_transactions lt
	JOIN journal_entries je ON lt.journal_entry_id = je.id
	WHERE (lt.debit_account_code = p_account_code OR lt.credit_account_code = p_account_code)
	  AND je.is_posted = true
	  AND (p_start_date IS NULL OR lt.transaction_date >= p_start_date)
	  AND (p_end_date IS NULL OR lt.transaction_date <= p_end_date)
	ORDER BY lt.transaction_date, je.entry_number
	LIMIT p_limit
    )
    SELECT
	am.transaction_date,
	am.entry_number,
	am.description,
	am.debit_amount,
	am.credit_amount,
	am.currency,
	SUM(am.net_amount) OVER (ORDER BY am.transaction_date, am.entry_number
				ROWS UNBOUNDED PRECEDING) as running_balance
    FROM account_movements am;
END;
$$ LANGUAGE plpgsql;

-- 6. BATCH OPERATIONS
-- ============================================================================

-- Create simple journal entry (single debit/credit)
CREATE OR REPLACE FUNCTION create_simple_entry(
    p_entry_number VARCHAR(50),
    p_entry_date DATE,
    p_debit_account VARCHAR(20),
    p_credit_account VARCHAR(20),
    p_amount DECIMAL(20,4),
    p_currency CHAR(3) DEFAULT 'USD',
    p_description TEXT DEFAULT NULL,
    p_reference VARCHAR(100) DEFAULT NULL,
    p_auto_post BOOLEAN DEFAULT false
) RETURNS UUID AS $$
DECLARE
    v_entry_id UUID;
BEGIN
    -- Create journal entry
    v_entry_id := create_journal_entry(
	p_entry_number, p_entry_date, p_description, p_reference, 'SYSTEM'
    );

    -- Create transaction
    PERFORM create_ledger_transaction(
	v_entry_id, p_entry_date,
	p_debit_account, p_amount, p_currency,
	p_credit_account, p_amount, p_currency,
	p_currency, p_description, p_reference
    );

    -- Auto-post if requested
    IF p_auto_post THEN
	PERFORM post_journal_entry(v_entry_id);
    END IF;

    RETURN v_entry_id;
END;
$$ LANGUAGE plpgsql;

-- Close accounting period (transfer revenue/expense to retained earnings)
CREATE OR REPLACE FUNCTION close_accounting_period(
    p_period_end_date DATE,
    p_retained_earnings_account VARCHAR(20) DEFAULT '3100',
    p_base_currency CHAR(3) DEFAULT 'USD'
) RETURNS UUID AS $$
DECLARE
    v_entry_id UUID;
    v_net_income DECIMAL(20,4);
    v_entry_number VARCHAR(50);
BEGIN
    -- Calculate net income
    SELECT SUM(
	CASE WHEN coa.account_type = 'REVENUE'
	     THEN ab.credit_balance - ab.debit_balance
	     ELSE ab.debit_balance - ab.credit_balance
	END
    ) INTO v_net_income
    FROM get_trial_balance(p_period_end_date, p_base_currency) ab
    JOIN chart_of_accounts coa ON ab.account_code = coa.account_code
    WHERE coa.account_type IN ('REVENUE', 'EXPENSE');

    v_entry_number := 'CLOSE-' || TO_CHAR(p_period_end_date, 'YYYY-MM');

    -- Create closing entry
    IF v_net_income > 0 THEN
	-- Profit: Credit retained earnings
	v_entry_id := create_simple_entry(
	    v_entry_number, p_period_end_date,
	    '9999', p_retained_earnings_account, -- Temporary income summary account
	    v_net_income, p_base_currency,
	    'Period closing - Net Income', 'CLOSE', true
	);
    ELSIF v_net_income < 0 THEN
	-- Loss: Debit retained earnings
	v_entry_id := create_simple_entry(
	    v_entry_number, p_period_end_date,
	    p_retained_earnings_account, '9999',
	    ABS(v_net_income), p_base_currency,
	    'Period closing - Net Loss', 'CLOSE', true
	);
    END IF;

    RETURN v_entry_id;
END;
$$ LANGUAGE plpgsql;

-- ============================================================================
-- COMPREHENSIVE BENCHMARKING SUITE FOR DOUBLE-ENTRY ACCOUNTING SCHEMA
-- ============================================================================

-- Drop the table
DROP TABLE IF EXISTS benchmark_runs CASCADE;

-- Benchmark configuration and results tables
CREATE TABLE IF NOT EXISTS benchmark_runs (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    run_name VARCHAR(100) NOT NULL,
    scale_factor INTEGER NOT NULL,
    duration_seconds INTEGER NOT NULL,
    verbosity_level INTEGER NOT NULL,
    start_time TIMESTAMPTZ NOT NULL,
    end_time TIMESTAMPTZ,
    total_operations INTEGER DEFAULT 0,
    total_errors INTEGER DEFAULT 0,
    status VARCHAR(20) DEFAULT 'RUNNING',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Drop the table
DROP TABLE IF EXISTS benchmark_metrics CASCADE;

CREATE TABLE IF NOT EXISTS benchmark_metrics (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    run_id UUID NOT NULL REFERENCES benchmark_runs(id) ON DELETE CASCADE,
    operation_type VARCHAR(50) NOT NULL,
    operation_count INTEGER NOT NULL DEFAULT 0,
    total_duration_ms BIGINT NOT NULL DEFAULT 0,
    min_duration_ms BIGINT NOT NULL DEFAULT 0,
    max_duration_ms BIGINT NOT NULL DEFAULT 0,
    avg_duration_ms DECIMAL(10,2) NOT NULL DEFAULT 0,
    p95_duration_ms BIGINT NOT NULL DEFAULT 0,
    p99_duration_ms BIGINT NOT NULL DEFAULT 0,
    errors INTEGER NOT NULL DEFAULT 0,
    throughput_ops_per_sec DECIMAL(10,2) NOT NULL DEFAULT 0
);

-- Drop the table
DROP TABLE IF EXISTS operation_timings CASCADE;

-- Create regular table for storing individual operation timings
CREATE TABLE IF NOT EXISTS operation_timings (
    operation_type VARCHAR(50),
    duration_ms BIGINT,
    success BOOLEAN,
    timestamp TIMESTAMPTZ DEFAULT NOW()
);

-- ============================================================================
-- BENCHMARK DATA GENERATION FUNCTIONS
-- ============================================================================

-- Generate test accounts
CREATE OR REPLACE FUNCTION generate_test_accounts(p_count INTEGER)
RETURNS INTEGER AS $$
DECLARE
    i INTEGER;
    v_account_code VARCHAR(20);
    v_account_types VARCHAR(20)[] := ARRAY['ASSET', 'LIABILITY', 'EQUITY', 'REVENUE', 'EXPENSE'];
    v_normal_balances VARCHAR(6)[] := ARRAY['DEBIT', 'CREDIT', 'CREDIT', 'CREDIT', 'DEBIT'];
BEGIN
    FOR i IN 1..p_count LOOP
	v_account_code := 'TEST' || LPAD(i::TEXT, 6, '0');

	INSERT INTO chart_of_accounts (
	    account_code,
	    account_name,
	    account_type,
	    normal_balance
	) VALUES (
	    v_account_code,
	    'Test Account ' || i,
	    v_account_types[1 + (i % 5)],
	    v_normal_balances[1 + (i % 5)]
	) ON CONFLICT (account_code) DO NOTHING;
    END LOOP;

    RETURN p_count;
END;
$$ LANGUAGE plpgsql;

-- Generate test currencies and exchange rates
CREATE OR REPLACE FUNCTION generate_test_currencies()
RETURNS INTEGER AS $$
DECLARE
    v_currencies CHAR(3)[] := ARRAY['USD', 'EUR', 'GBP', 'JPY', 'CAD', 'AUD', 'CHF', 'CNY'];
    v_rates DECIMAL(10,4)[] := ARRAY[1.0000, 0.8500, 0.7500, 110.0000, 1.2500, 1.3500, 0.9200, 6.4500];
    i INTEGER;
    j INTEGER;
BEGIN
    -- Insert currencies
    FOR i IN 1..array_length(v_currencies, 1) LOOP
	INSERT INTO currencies (currency_code, currency_name, decimal_places)
	VALUES (v_currencies[i], v_currencies[i] || ' Currency',
		CASE WHEN v_currencies[i] = 'JPY' THEN 0 ELSE 2 END)
	ON CONFLICT (currency_code) DO NOTHING;
    END LOOP;

    -- Insert exchange rates (all to USD)
    FOR i IN 1..array_length(v_currencies, 1) LOOP
	FOR j IN 1..array_length(v_currencies, 1) LOOP
	    IF i != j THEN
		INSERT INTO exchange_rates (from_currency, to_currency, rate, effective_date)
		VALUES (v_currencies[i], v_currencies[j],
			v_rates[j] / v_rates[i], CURRENT_DATE)
		ON CONFLICT (from_currency, to_currency, effective_date) DO NOTHING;
	    END IF;
	END LOOP;
    END LOOP;

    RETURN array_length(v_currencies, 1);
END;
$$ LANGUAGE plpgsql;

-- ============================================================================
-- BENCHMARK OPERATION FUNCTIONS
-- ============================================================================

-- Benchmark journal entry creation
CREATE OR REPLACE FUNCTION benchmark_journal_entry_creation(
    p_iterations INTEGER,
    p_verbosity INTEGER DEFAULT 0
) RETURNS TABLE(
    operation_type VARCHAR(50),
    total_ops INTEGER,
    total_time_ms BIGINT,
    avg_time_ms DECIMAL(10,2),
    min_time_ms BIGINT,
    max_time_ms BIGINT,
    errors INTEGER
) AS $$
DECLARE
    i INTEGER;
    v_start_time TIMESTAMPTZ;
    v_end_time TIMESTAMPTZ;
    v_duration_ms BIGINT;
    v_entry_id UUID;
    v_errors INTEGER := 0;
    v_total_ops INTEGER;
    v_total_time_ms BIGINT;
    v_avg_time_ms DECIMAL(10,2);
    v_min_time_ms BIGINT;
    v_max_time_ms BIGINT;
BEGIN
    DELETE FROM operation_timings WHERE operation_timings.operation_type = 'journal_entry_creation';

    FOR i IN 1..p_iterations LOOP
	BEGIN
	    v_start_time := clock_timestamp();

	    v_entry_id := create_journal_entry(
		'BENCH-JE-' || i || '-' || extract(epoch from now())::bigint,
		CURRENT_DATE,
		'Benchmark Journal Entry ' || i,
		'BENCH-REF-' || i,
		'BENCHMARK'
	    );

	    v_end_time := clock_timestamp();
	    v_duration_ms := extract(milliseconds from (v_end_time - v_start_time))::bigint;

	    INSERT INTO operation_timings VALUES ('journal_entry_creation', v_duration_ms, true);

	    IF p_verbosity >= 2 AND i % 100 = 0 THEN
		RAISE NOTICE 'Created journal entry %: % (% ms)', i, v_entry_id, v_duration_ms;
	    END IF;

	EXCEPTION WHEN OTHERS THEN
	    v_errors := v_errors + 1;
	    INSERT INTO operation_timings VALUES ('journal_entry_creation', 0, false);

	    IF p_verbosity >= 1 THEN
		RAISE NOTICE 'Error creating journal entry %: %', i, SQLERRM;
	    END IF;
	END;
    END LOOP;

    -- Get aggregated results
    SELECT
	COUNT(*)::INTEGER,
	SUM(ot.duration_ms),
	AVG(ot.duration_ms)::DECIMAL(10,2),
	MIN(ot.duration_ms),
	MAX(ot.duration_ms)
    INTO v_total_ops, v_total_time_ms, v_avg_time_ms, v_min_time_ms, v_max_time_ms
    FROM operation_timings ot
    WHERE ot.operation_type = 'journal_entry_creation' AND ot.success = true;

    -- Return single row
    operation_type := 'journal_entry_creation';
    total_ops := COALESCE(v_total_ops, 0);
    total_time_ms := COALESCE(v_total_time_ms, 0);
    avg_time_ms := COALESCE(v_avg_time_ms, 0);
    min_time_ms := COALESCE(v_min_time_ms, 0);
    max_time_ms := COALESCE(v_max_time_ms, 0);
    errors := v_errors;

    RETURN NEXT;
END;
$$ LANGUAGE plpgsql;

-- Benchmark transaction creation
CREATE OR REPLACE FUNCTION benchmark_transaction_creation(
    p_iterations INTEGER,
    p_verbosity INTEGER DEFAULT 0
) RETURNS TABLE(
    operation_type VARCHAR(50),
    total_ops INTEGER,
    total_time_ms BIGINT,
    avg_time_ms DECIMAL(10,2),
    min_time_ms BIGINT,
    max_time_ms BIGINT,
    errors INTEGER
) AS $$
DECLARE
    i INTEGER;
    v_start_time TIMESTAMPTZ;
    v_end_time TIMESTAMPTZ;
    v_duration_ms BIGINT;
    v_entry_id UUID;
    v_transaction_id UUID;
    v_debit_account VARCHAR(20);
    v_credit_account VARCHAR(20);
    v_amount DECIMAL(20,4);
    v_currency CHAR(3);
    v_currencies CHAR(3)[] := ARRAY['USD', 'EUR', 'GBP', 'JPY', 'CAD'];
    v_accounts VARCHAR(20)[];
    v_errors INTEGER := 0;
    v_total_ops INTEGER;
    v_total_time_ms BIGINT;
    v_avg_time_ms DECIMAL(10,2);
    v_min_time_ms BIGINT;
    v_max_time_ms BIGINT;
BEGIN
    DELETE FROM operation_timings WHERE operation_timings.operation_type = 'transaction_creation';

    -- Get available test accounts
    SELECT array_agg(coa.account_code) INTO v_accounts
    FROM chart_of_accounts coa
    WHERE coa.account_code LIKE 'TEST%' AND coa.is_leaf = true
    LIMIT 100;

    IF array_length(v_accounts, 1) < 2 THEN
	RAISE EXCEPTION 'Insufficient test accounts. Run generate_test_accounts() first.';
    END IF;

    FOR i IN 1..p_iterations LOOP
	BEGIN
	    -- Create journal entry first
	    v_entry_id := create_journal_entry(
		'BENCH-TX-' || i || '-' || extract(epoch from now())::bigint,
		CURRENT_DATE,
		'Benchmark Transaction ' || i,
		'BENCH-TX-REF-' || i,
		'BENCHMARK'
	    );

	    -- Random transaction parameters
	    v_debit_account := v_accounts[1 + (random() * (array_length(v_accounts, 1) - 1))::integer];
	    v_credit_account := v_accounts[1 + (random() * (array_length(v_accounts, 1) - 1))::integer];

	    -- Ensure different accounts
	    WHILE v_debit_account = v_credit_account LOOP
		v_credit_account := v_accounts[1 + (random() * (array_length(v_accounts, 1) - 1))::integer];
	    END LOOP;

	    v_amount := (random() * 10000 + 1)::DECIMAL(20,4);
	    v_currency := v_currencies[1 + (random() * (array_length(v_currencies, 1) - 1))::integer];

	    v_start_time := clock_timestamp();

	    v_transaction_id := create_ledger_transaction(
		v_entry_id,
		CURRENT_DATE,
		v_debit_account,
		v_amount,
		v_currency,
		v_credit_account,
		v_amount,
		v_currency,
		'USD',
		'Benchmark transaction ' || i,
		'BENCH-TX-' || i
	    );

	    v_end_time := clock_timestamp();
	    v_duration_ms := extract(milliseconds from (v_end_time - v_start_time))::bigint;

	    INSERT INTO operation_timings VALUES ('transaction_creation', v_duration_ms, true);

	    IF p_verbosity >= 2 AND i % 100 = 0 THEN
		RAISE NOTICE 'Created transaction %: % (% ms)', i, v_transaction_id, v_duration_ms;
	    END IF;

	EXCEPTION WHEN OTHERS THEN
	    v_errors := v_errors + 1;
	    INSERT INTO operation_timings VALUES ('transaction_creation', 0, false);

	    IF p_verbosity >= 1 THEN
		RAISE NOTICE 'Error creating transaction %: %', i, SQLERRM;
	    END IF;
	END;
    END LOOP;

    -- Get aggregated results
    SELECT
	COUNT(*)::INTEGER,
	SUM(ot.duration_ms),
	AVG(ot.duration_ms)::DECIMAL(10,2),
	MIN(ot.duration_ms),
	MAX(ot.duration_ms)
    INTO v_total_ops, v_total_time_ms, v_avg_time_ms, v_min_time_ms, v_max_time_ms
    FROM operation_timings ot
    WHERE ot.operation_type = 'transaction_creation' AND ot.success = true;

    -- Return single row
    operation_type := 'transaction_creation';
    total_ops := COALESCE(v_total_ops, 0);
    total_time_ms := COALESCE(v_total_time_ms, 0);
    avg_time_ms := COALESCE(v_avg_time_ms, 0);
    min_time_ms := COALESCE(v_min_time_ms, 0);
    max_time_ms := COALESCE(v_max_time_ms, 0);
    errors := v_errors;

    RETURN NEXT;
END;
$$ LANGUAGE plpgsql;

-- Benchmark posting operations
CREATE OR REPLACE FUNCTION benchmark_posting_operations(
    p_iterations INTEGER,
    p_verbosity INTEGER DEFAULT 0
) RETURNS TABLE(
    operation_type VARCHAR(50),
    total_ops INTEGER,
    total_time_ms BIGINT,
    avg_time_ms DECIMAL(10,2),
    min_time_ms BIGINT,
    max_time_ms BIGINT,
    errors INTEGER
) AS $$
DECLARE
    i INTEGER;
    v_start_time TIMESTAMPTZ;
    v_end_time TIMESTAMPTZ;
    v_duration_ms BIGINT;
    v_entry_id UUID;
    v_unposted_entries UUID[];
    v_errors INTEGER := 0;
    v_total_ops INTEGER;
    v_total_time_ms BIGINT;
    v_avg_time_ms DECIMAL(10,2);
    v_min_time_ms BIGINT;
    v_max_time_ms BIGINT;
BEGIN
    DELETE FROM operation_timings WHERE operation_timings.operation_type = 'posting_operations';

    -- Get unposted entries - Fixed query
    SELECT array_agg(id) INTO v_unposted_entries
    FROM (
        SELECT je.id
        FROM journal_entries je
        WHERE je.is_posted = false
        ORDER BY je.created_at
        LIMIT p_iterations
    ) subq;

    IF array_length(v_unposted_entries, 1) IS NULL THEN
        RAISE NOTICE 'No unposted entries available';
        -- Return empty result
        operation_type := 'posting_operations';
        total_ops := 0;
        total_time_ms := 0;
        avg_time_ms := 0;
        min_time_ms := 0;
        max_time_ms := 0;
        errors := 0;
        RETURN NEXT;
        RETURN;
    END IF;

    IF array_length(v_unposted_entries, 1) < p_iterations THEN
        RAISE NOTICE 'Only % unposted entries available, expected %',
                     array_length(v_unposted_entries, 1), p_iterations;
    END IF;

    FOR i IN 1..LEAST(p_iterations, array_length(v_unposted_entries, 1)) LOOP
        BEGIN
            v_entry_id := v_unposted_entries[i];

            v_start_time := clock_timestamp();

            PERFORM post_journal_entry(v_entry_id);

            v_end_time := clock_timestamp();
            v_duration_ms := extract(milliseconds from (v_end_time - v_start_time))::bigint;

            INSERT INTO operation_timings VALUES ('posting_operations', v_duration_ms, true);

            IF p_verbosity >= 2 AND i % 100 = 0 THEN
                RAISE NOTICE 'Posted entry %: % (% ms)', i, v_entry_id, v_duration_ms;
            END IF;

        EXCEPTION WHEN OTHERS THEN
            v_errors := v_errors + 1;
            INSERT INTO operation_timings VALUES ('posting_operations', 0, false);

            IF p_verbosity >= 1 THEN
                RAISE NOTICE 'Error posting entry %: %', i, SQLERRM;
            END IF;
        END;
    END LOOP;

    -- Get aggregated results
    SELECT 
        COUNT(*)::INTEGER,
        SUM(ot.duration_ms),
        AVG(ot.duration_ms)::DECIMAL(10,2),
        MIN(ot.duration_ms),
        MAX(ot.duration_ms)
    INTO v_total_ops, v_total_time_ms, v_avg_time_ms, v_min_time_ms, v_max_time_ms
    FROM operation_timings ot
    WHERE ot.operation_type = 'posting_operations' AND ot.success = true;

    -- Return single row
    operation_type := 'posting_operations';
    total_ops := COALESCE(v_total_ops, 0);
    total_time_ms := COALESCE(v_total_time_ms, 0);
    avg_time_ms := COALESCE(v_avg_time_ms, 0);
    min_time_ms := COALESCE(v_min_time_ms, 0);
    max_time_ms := COALESCE(v_max_time_ms, 0);
    errors := v_errors;

    RETURN NEXT;
END;
$$ LANGUAGE plpgsql;

-- Benchmark balance queries
CREATE OR REPLACE FUNCTION benchmark_balance_queries(
    p_iterations INTEGER,
    p_verbosity INTEGER DEFAULT 0
) RETURNS TABLE(
    operation_type VARCHAR(50),
    total_ops INTEGER,
    total_time_ms BIGINT,
    avg_time_ms DECIMAL(10,2),
    min_time_ms BIGINT,
    max_time_ms BIGINT,
    errors INTEGER
) AS $$
DECLARE
    i INTEGER;
    v_start_time TIMESTAMPTZ;
    v_end_time TIMESTAMPTZ;
    v_duration_ms BIGINT;
    v_account_code VARCHAR(20);
    v_accounts VARCHAR(20)[];
    v_balance_result RECORD;
    v_errors INTEGER := 0;
    v_total_ops INTEGER;
    v_total_time_ms BIGINT;
    v_avg_time_ms DECIMAL(10,2);
    v_min_time_ms BIGINT;
    v_max_time_ms BIGINT;
BEGIN
    DELETE FROM operation_timings WHERE operation_timings.operation_type = 'balance_queries';

    -- Get available accounts
    SELECT array_agg(coa.account_code) INTO v_accounts
    FROM chart_of_accounts coa
    WHERE coa.is_leaf = true
    LIMIT 100;

    FOR i IN 1..p_iterations LOOP
	BEGIN
	    v_account_code := v_accounts[1 + (random() * (array_length(v_accounts, 1) - 1))::integer];

	    v_start_time := clock_timestamp();

	    SELECT * INTO v_balance_result
	    FROM get_account_balance(v_account_code, NULL, CURRENT_DATE)
	    LIMIT 1;

	    v_end_time := clock_timestamp();
	    v_duration_ms := extract(milliseconds from (v_end_time - v_start_time))::bigint;

	    INSERT INTO operation_timings VALUES ('balance_queries', v_duration_ms, true);

	    IF p_verbosity >= 2 AND i % 100 = 0 THEN
		RAISE NOTICE 'Queried balance %: % (% ms)', i, v_account_code, v_duration_ms;
	    END IF;

	EXCEPTION WHEN OTHERS THEN
	    v_errors := v_errors + 1;
	    INSERT INTO operation_timings VALUES ('balance_queries', 0, false);

	    IF p_verbosity >= 1 THEN
		RAISE NOTICE 'Error querying balance %: %', i, SQLERRM;
	    END IF;
	END;
    END LOOP;

    -- Get aggregated results
    SELECT
	COUNT(*)::INTEGER,
	SUM(ot.duration_ms),
	AVG(ot.duration_ms)::DECIMAL(10,2),
	MIN(ot.duration_ms),
	MAX(ot.duration_ms)
    INTO v_total_ops, v_total_time_ms, v_avg_time_ms, v_min_time_ms, v_max_time_ms
    FROM operation_timings ot
    WHERE ot.operation_type = 'balance_queries' AND ot.success = true;

    -- Return single row
    operation_type := 'balance_queries';
    total_ops := COALESCE(v_total_ops, 0);
    total_time_ms := COALESCE(v_total_time_ms, 0);
    avg_time_ms := COALESCE(v_avg_time_ms, 0);
    min_time_ms := COALESCE(v_min_time_ms, 0);
    max_time_ms := COALESCE(v_max_time_ms, 0);
    errors := v_errors;

    RETURN NEXT;
END;
$$ LANGUAGE plpgsql;

-- Benchmark trial balance generation
CREATE OR REPLACE FUNCTION benchmark_trial_balance(
    p_iterations INTEGER,
    p_verbosity INTEGER DEFAULT 0
) RETURNS TABLE(
    operation_type VARCHAR(50),
    total_ops INTEGER,
    total_time_ms BIGINT,
    avg_time_ms DECIMAL(10,2),
    min_time_ms BIGINT,
    max_time_ms BIGINT,
    errors INTEGER
) AS $$
DECLARE
    i INTEGER;
    v_start_time TIMESTAMPTZ;
    v_end_time TIMESTAMPTZ;
    v_duration_ms BIGINT;
    v_trial_balance_count INTEGER;
    v_errors INTEGER := 0;
    v_total_ops INTEGER;
    v_total_time_ms BIGINT;
    v_avg_time_ms DECIMAL(10,2);
    v_min_time_ms BIGINT;
    v_max_time_ms BIGINT;
BEGIN
    DELETE FROM operation_timings WHERE operation_timings.operation_type = 'trial_balance';

    FOR i IN 1..p_iterations LOOP
	BEGIN
	    v_start_time := clock_timestamp();

	    SELECT COUNT(*) INTO v_trial_balance_count
	    FROM get_trial_balance(CURRENT_DATE, 'USD');

	    v_end_time := clock_timestamp();
	    v_duration_ms := extract(milliseconds from (v_end_time - v_start_time))::bigint;

	    INSERT INTO operation_timings VALUES ('trial_balance', v_duration_ms, true);

	    IF p_verbosity >= 2 AND i % 10 = 0 THEN
		RAISE NOTICE 'Generated trial balance %: % accounts (% ms)', i, v_trial_balance_count, v_duration_ms;
	    END IF;

	EXCEPTION WHEN OTHERS THEN
	    v_errors := v_errors + 1;
	    INSERT INTO operation_timings VALUES ('trial_balance', 0, false);

	    IF p_verbosity >= 1 THEN
		RAISE NOTICE 'Error generating trial balance %: %', i, SQLERRM;
	    END IF;
	END;
    END LOOP;

    -- Get aggregated results
    SELECT
	COUNT(*)::INTEGER,
	SUM(ot.duration_ms),
	AVG(ot.duration_ms)::DECIMAL(10,2),
	MIN(ot.duration_ms),
	MAX(ot.duration_ms)
    INTO v_total_ops, v_total_time_ms, v_avg_time_ms, v_min_time_ms, v_max_time_ms
    FROM operation_timings ot
    WHERE ot.operation_type = 'trial_balance' AND ot.success = true;

    -- Return single row
    operation_type := 'trial_balance';
    total_ops := COALESCE(v_total_ops, 0);
    total_time_ms := COALESCE(v_total_time_ms, 0);
    avg_time_ms := COALESCE(v_avg_time_ms, 0);
    min_time_ms := COALESCE(v_min_time_ms, 0);
    max_time_ms := COALESCE(v_max_time_ms, 0);
    errors := v_errors;

    RETURN NEXT;
END;
$$ LANGUAGE plpgsql;

-- ============================================================================
-- MAIN BENCHMARK FUNCTION
-- ============================================================================

CREATE OR REPLACE FUNCTION run_accounting_benchmark(
    p_scale_factor INTEGER DEFAULT 1000,
    p_duration_seconds INTEGER DEFAULT 300,
    p_verbosity INTEGER DEFAULT 1
) RETURNS UUID AS $$
DECLARE
    v_run_id UUID;
    v_start_time TIMESTAMPTZ;
    v_end_time TIMESTAMPTZ;
    v_current_time TIMESTAMPTZ;
    v_elapsed_seconds INTEGER;
    v_total_operations INTEGER := 0;
    v_total_errors INTEGER := 0;
    v_result RECORD;
    v_percentiles RECORD;
    v_time_exceeded BOOLEAN := false;

    -- Operation configurations based on scale factor
    v_je_iterations INTEGER := p_scale_factor;
    v_tx_iterations INTEGER := p_scale_factor * 2;
    v_post_iterations INTEGER := p_scale_factor;
    v_balance_iterations INTEGER := p_scale_factor / 2;
    v_trial_iterations INTEGER := GREATEST(p_scale_factor / 10, 10);
BEGIN
    v_start_time := clock_timestamp();

    -- Create benchmark run record
    INSERT INTO benchmark_runs (
	run_name, scale_factor, duration_seconds, verbosity_level, start_time
    ) VALUES (
	'Accounting Benchmark ' || to_char(v_start_time, 'YYYY-MM-DD HH24:MI:SS'),
	p_scale_factor, p_duration_seconds, p_verbosity, v_start_time
    ) RETURNING id INTO v_run_id;

    IF p_verbosity >= 1 THEN
	RAISE NOTICE '=== STARTING ACCOUNTING BENCHMARK ===';
	RAISE NOTICE 'Run ID: %', v_run_id;
	RAISE NOTICE 'Scale Factor: %', p_scale_factor;
	RAISE NOTICE 'Duration: % seconds', p_duration_seconds;
	RAISE NOTICE '';
    END IF;

    -- Setup test data
    IF p_verbosity >= 1 THEN
	RAISE NOTICE 'Setting up test data...';
    END IF;

    PERFORM generate_test_currencies();
    PERFORM generate_test_accounts(GREATEST(p_scale_factor / 10, 100));

    -- Clear previous timing data
    DELETE FROM operation_timings;

    -- Phase 1: Journal Entry Creation
    IF NOT v_time_exceeded THEN
	IF p_verbosity >= 1 THEN
	    RAISE NOTICE 'Phase 1: Creating % journal entries...', v_je_iterations;
	END IF;

	SELECT * INTO v_result FROM benchmark_journal_entry_creation(v_je_iterations, p_verbosity);

	INSERT INTO benchmark_metrics (
	    run_id, operation_type, operation_count, total_duration_ms,
	    min_duration_ms, max_duration_ms, avg_duration_ms, errors
	) VALUES (
	    v_run_id, v_result.operation_type, v_result.total_ops, v_result.total_time_ms,
	    v_result.min_time_ms, v_result.max_time_ms, v_result.avg_time_ms, v_result.errors
	);

	v_total_operations := v_total_operations + v_result.total_ops;
	v_total_errors := v_total_errors + v_result.errors;

	-- Check time limit
	v_current_time := clock_timestamp();
	v_elapsed_seconds := extract(epoch from (v_current_time - v_start_time))::integer;
	v_time_exceeded := (v_elapsed_seconds >= p_duration_seconds);
    END IF;

    -- Phase 2: Transaction Creation
    IF NOT v_time_exceeded THEN
	IF p_verbosity >= 1 THEN
	    RAISE NOTICE 'Phase 2: Creating % transactions...', v_tx_iterations;
	END IF;

	SELECT * INTO v_result FROM benchmark_transaction_creation(v_tx_iterations, p_verbosity);

	INSERT INTO benchmark_metrics (
	    run_id, operation_type, operation_count, total_duration_ms,
	    min_duration_ms, max_duration_ms, avg_duration_ms, errors
	) VALUES (
	    v_run_id, v_result.operation_type, v_result.total_ops, v_result.total_time_ms,
	    v_result.min_time_ms, v_result.max_time_ms, v_result.avg_time_ms, v_result.errors
	);

	v_total_operations := v_total_operations + v_result.total_ops;
	v_total_errors := v_total_errors + v_result.errors;

	-- Check time limit
	v_current_time := clock_timestamp();
	v_elapsed_seconds := extract(epoch from (v_current_time - v_start_time))::integer;
	v_time_exceeded := (v_elapsed_seconds >= p_duration_seconds);
    END IF;

    -- Phase 3: Posting Operations
    IF NOT v_time_exceeded THEN
	IF p_verbosity >= 1 THEN
	    RAISE NOTICE 'Phase 3: Posting % entries...', v_post_iterations;
	END IF;

	SELECT * INTO v_result FROM benchmark_posting_operations(v_post_iterations, p_verbosity);

	INSERT INTO benchmark_metrics (
	    run_id, operation_type, operation_count, total_duration_ms,
	    min_duration_ms, max_duration_ms, avg_duration_ms, errors
	) VALUES (
	    v_run_id, v_result.operation_type, v_result.total_ops, v_result.total_time_ms,
	    v_result.min_time_ms, v_result.max_time_ms, v_result.avg_time_ms, v_result.errors
	);

	v_total_operations := v_total_operations + v_result.total_ops;
	v_total_errors := v_total_errors + v_result.errors;

	-- Check time limit
	v_current_time := clock_timestamp();
	v_elapsed_seconds := extract(epoch from (v_current_time - v_start_time))::integer;
	v_time_exceeded := (v_elapsed_seconds >= p_duration_seconds);
    END IF;

    -- Phase 4: Balance Queries
    IF NOT v_time_exceeded THEN
	IF p_verbosity >= 1 THEN
	    RAISE NOTICE 'Phase 4: Running % balance queries...', v_balance_iterations;
	END IF;

	SELECT * INTO v_result FROM benchmark_balance_queries(v_balance_iterations, p_verbosity);

	INSERT INTO benchmark_metrics (
	    run_id, operation_type, operation_count, total_duration_ms,
	    min_duration_ms, max_duration_ms, avg_duration_ms, errors
	) VALUES (
	    v_run_id, v_result.operation_type, v_result.total_ops, v_result.total_time_ms,
	    v_result.min_time_ms, v_result.max_time_ms, v_result.avg_time_ms, v_result.errors
	);

	v_total_operations := v_total_operations + v_result.total_ops;
	v_total_errors := v_total_errors + v_result.errors;

	-- Check time limit
	v_current_time := clock_timestamp();
	v_elapsed_seconds := extract(epoch from (v_current_time - v_start_time))::integer;
	v_time_exceeded := (v_elapsed_seconds >= p_duration_seconds);
    END IF;

    -- Phase 5: Trial Balance Generation
    IF NOT v_time_exceeded THEN
	IF p_verbosity >= 1 THEN
	    RAISE NOTICE 'Phase 5: Generating % trial balances...', v_trial_iterations;
	END IF;

	SELECT * INTO v_result FROM benchmark_trial_balance(v_trial_iterations, p_verbosity);

	INSERT INTO benchmark_metrics (
	    run_id, operation_type, operation_count, total_duration_ms,
	    min_duration_ms, max_duration_ms, avg_duration_ms, errors
	) VALUES (
	    v_run_id, v_result.operation_type, v_result.total_ops, v_result.total_time_ms,
	    v_result.min_time_ms, v_result.max_time_ms, v_result.avg_time_ms, v_result.errors
	);

	v_total_operations := v_total_operations + v_result.total_ops;
	v_total_errors := v_total_errors + v_result.errors;
    END IF;

    -- Finalize benchmark
    v_end_time := clock_timestamp();
    v_elapsed_seconds := extract(epoch from (v_end_time - v_start_time))::integer;

    -- Calculate percentiles and throughput for each operation type
    FOR v_result IN
	SELECT DISTINCT bm.operation_type FROM benchmark_metrics bm WHERE bm.run_id = v_run_id
    LOOP
	-- Calculate percentiles from operation_timings
	SELECT
	    percentile_cont(0.95) WITHIN GROUP (ORDER BY ot.duration_ms) as p95,
	    percentile_cont(0.99) WITHIN GROUP (ORDER BY ot.duration_ms) as p99
	INTO v_percentiles
	FROM operation_timings ot
	WHERE ot.operation_type = v_result.operation_type AND ot.success = true;

	-- Update metrics with percentiles and throughput
	UPDATE benchmark_metrics
	SET
	    p95_duration_ms = COALESCE(v_percentiles.p95::BIGINT, 0),
	    p99_duration_ms = COALESCE(v_percentiles.p99::BIGINT, 0),
	    throughput_ops_per_sec = CASE
		WHEN total_duration_ms > 0 THEN (operation_count * 1000.0) / total_duration_ms
		ELSE 0
	    END
	WHERE run_id = v_run_id AND operation_type = v_result.operation_type;
    END LOOP;

    -- Update run record
    UPDATE benchmark_runs
    SET
	end_time = v_end_time,
	total_operations = v_total_operations,
	total_errors = v_total_errors,
	status = 'COMPLETED'
    WHERE id = v_run_id;

    -- Display results
    IF p_verbosity >= 1 THEN
	RAISE NOTICE '';
	RAISE NOTICE '=== BENCHMARK RESULTS ===';
	RAISE NOTICE 'Total Duration: % seconds', v_elapsed_seconds;
	RAISE NOTICE 'Total Operations: %', v_total_operations;
	RAISE NOTICE 'Total Errors: %', v_total_errors;
	RAISE NOTICE 'Overall Throughput: % ops/sec',
		     CASE WHEN v_elapsed_seconds > 0
			  THEN ROUND((v_total_operations::DECIMAL / v_elapsed_seconds), 2)
			  ELSE 0 END;
	RAISE NOTICE '';

	-- Detailed metrics
	FOR v_result IN
	    SELECT
		bm.operation_type,
		bm.operation_count,
		bm.avg_duration_ms,
		bm.min_duration_ms,
		bm.max_duration_ms,
		bm.p95_duration_ms,
		bm.p99_duration_ms,
		bm.throughput_ops_per_sec,
		bm.errors
	    FROM benchmark_metrics bm
	    WHERE bm.run_id = v_run_id
	    ORDER BY bm.operation_type
	LOOP
	    RAISE NOTICE '--- % ---', upper(replace(v_result.operation_type, '_', ' '));
	    RAISE NOTICE 'Operations: %', v_result.operation_count;
	    RAISE NOTICE 'Avg Duration: % ms', v_result.avg_duration_ms;
	    RAISE NOTICE 'Min/Max Duration: %/% ms', v_result.min_duration_ms, v_result.max_duration_ms;
	    RAISE NOTICE 'P95/P99 Duration: %/% ms', v_result.p95_duration_ms, v_result.p99_duration_ms;
	    RAISE NOTICE 'Throughput: % ops/sec', v_result.throughput_ops_per_sec;
	    RAISE NOTICE 'Errors: %', v_result.errors;
	    RAISE NOTICE '';
	END LOOP;

	IF v_time_exceeded THEN
	    RAISE NOTICE 'NOTE: Benchmark stopped due to time limit of % seconds', p_duration_seconds;
	END IF;
    END IF;

    RETURN v_run_id;

EXCEPTION WHEN OTHERS THEN
    -- Update run record with error status
    UPDATE benchmark_runs
    SET
	end_time = clock_timestamp(),
	status = 'ERROR',
	total_operations = v_total_operations,
	total_errors = v_total_errors + 1
    WHERE id = v_run_id;

    RAISE NOTICE 'Benchmark failed: %', SQLERRM;
    RETURN v_run_id;
END;
$$ LANGUAGE plpgsql;

-- ============================================================================
-- ADDITIONAL HELPER FUNCTION FOR CLEANUP
-- ============================================================================

-- Clean up test data after benchmarking
CREATE OR REPLACE FUNCTION cleanup_benchmark_data()
RETURNS BOOLEAN AS $$
BEGIN
    -- Delete test transactions and journal entries
    DELETE FROM ledger_transactions
    WHERE journal_entry_id IN (
	SELECT id FROM journal_entries
	WHERE created_by = 'BENCHMARK' OR entry_number LIKE 'BENCH-%'
    );

    DELETE FROM journal_entries
    WHERE created_by = 'BENCHMARK' OR entry_number LIKE 'BENCH-%';

    -- Delete test accounts
    DELETE FROM chart_of_accounts
    WHERE account_code LIKE 'TEST%';

    -- Refresh materialized view
    REFRESH MATERIALIZED VIEW CONCURRENTLY account_balances;

    RAISE NOTICE 'Benchmark test data cleaned up successfully';
    RETURN true;

EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'Error cleaning up benchmark data: %', SQLERRM;
    RETURN false;
END;
$$ LANGUAGE plpgsql;

-- ============================================================================
-- BENCHMARK ANALYSIS FUNCTIONS
-- ============================================================================

-- Get benchmark summary
CREATE OR REPLACE FUNCTION get_benchmark_summary(p_run_id UUID DEFAULT NULL)
RETURNS TABLE(
    run_id UUID,
    run_name VARCHAR(100),
    scale_factor INTEGER,
    duration_seconds INTEGER,
    actual_duration_seconds INTEGER,
    total_operations INTEGER,
    total_errors INTEGER,
    overall_throughput DECIMAL(10,2),
    status VARCHAR(20)
) AS $$
BEGIN
    RETURN QUERY
    SELECT
	br.id,
	br.run_name,
	br.scale_factor,
	br.duration_seconds,
	extract(epoch from (br.end_time - br.start_time))::integer,
	br.total_operations,
	br.total_errors,
	CASE
	    WHEN extract(epoch from (br.end_time - br.start_time)) > 0
	    THEN br.total_operations::DECIMAL / extract(epoch from (br.end_time - br.start_time))
	    ELSE 0
	END,
	br.status
    FROM benchmark_runs br
    WHERE (p_run_id IS NULL OR br.id = p_run_id)
    ORDER BY br.start_time DESC;
END;
$$ LANGUAGE plpgsql;

-- Compare benchmark runs
CREATE OR REPLACE FUNCTION compare_benchmark_runs(
    p_run_id_1 UUID,
    p_run_id_2 UUID
) RETURNS TABLE(
    operation_type VARCHAR(50),
    run1_throughput DECIMAL(10,2),
    run2_throughput DECIMAL(10,2),
    throughput_change_pct DECIMAL(10,2),
    run1_avg_ms DECIMAL(10,2),
    run2_avg_ms DECIMAL(10,2),
    latency_change_pct DECIMAL(10,2)
) AS $$
BEGIN
    RETURN QUERY
    SELECT
	COALESCE(m1.operation_type, m2.operation_type) as operation_type,
	COALESCE(m1.throughput_ops_per_sec, 0) as run1_throughput,
	COALESCE(m2.throughput_ops_per_sec, 0) as run2_throughput,
	CASE
	    WHEN m1.throughput_ops_per_sec > 0
	    THEN ((m2.throughput_ops_per_sec - m1.throughput_ops_per_sec) / m1.throughput_ops_per_sec * 100)
	    ELSE NULL
	END as throughput_change_pct,
	COALESCE(m1.avg_duration_ms, 0) as run1_avg_ms,
	COALESCE(m2.avg_duration_ms, 0) as run2_avg_ms,
	CASE
	    WHEN m1.avg_duration_ms > 0
	    THEN ((m2.avg_duration_ms - m1.avg_duration_ms) / m1.avg_duration_ms * 100)
	    ELSE NULL
	END as latency_change_pct
    FROM benchmark_metrics m1
    FULL OUTER JOIN benchmark_metrics m2 ON m1.operation_type = m2.operation_type
    WHERE m1.run_id = p_run_id_1 AND m2.run_id = p_run_id_2
    ORDER BY operation_type;
END;
$$ LANGUAGE plpgsql;
