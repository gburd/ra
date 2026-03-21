-- Representative financial dataset for interactive query optimization demos.
-- Designed to show how Ra's optimizer behavior changes with different
-- table sizes, cardinalities, and index configurations.
--
-- Schema intentionally simplified from the full ledger.sql to focus on
-- optimization concepts rather than accounting correctness.

-- ============================================================================
-- SCHEMA
-- ============================================================================

CREATE TABLE accounts (
    account_id INTEGER PRIMARY KEY,
    account_code VARCHAR(20) NOT NULL UNIQUE,
    account_name VARCHAR(100) NOT NULL,
    account_type VARCHAR(20) NOT NULL
        CHECK (account_type IN (
            'ASSET', 'LIABILITY', 'EQUITY', 'REVENUE', 'EXPENSE'
        )),
    normal_balance VARCHAR(6) NOT NULL
        CHECK (normal_balance IN ('DEBIT', 'CREDIT')),
    is_active BOOLEAN NOT NULL DEFAULT true,
    created_date DATE NOT NULL
);

CREATE TABLE transactions (
    transaction_id INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL
        REFERENCES accounts(account_id),
    transaction_date DATE NOT NULL,
    amount DECIMAL(10,2) NOT NULL,
    category VARCHAR(50) NOT NULL,
    description VARCHAR(200),
    entry_type VARCHAR(6) NOT NULL
        CHECK (entry_type IN ('DEBIT', 'CREDIT'))
);

CREATE TABLE categories (
    category_id INTEGER PRIMARY KEY,
    category_name VARCHAR(50) NOT NULL UNIQUE,
    category_type VARCHAR(20) NOT NULL
        CHECK (category_type IN ('income', 'expense', 'transfer'))
);

-- ============================================================================
-- INDEXES (toggled in the interactive demo)
-- ============================================================================

CREATE INDEX idx_txn_date ON transactions(transaction_date);
CREATE INDEX idx_txn_account ON transactions(account_id);
CREATE INDEX idx_txn_category ON transactions(category);
CREATE INDEX idx_txn_amount ON transactions(amount);
CREATE INDEX idx_txn_date_category
    ON transactions(transaction_date, category);
CREATE INDEX idx_accounts_type ON accounts(account_type);

-- ============================================================================
-- REFERENCE DATA: categories (20 rows)
-- ============================================================================

INSERT INTO categories (category_id, category_name, category_type) VALUES
( 1, 'Sales Revenue',       'income'),
( 2, 'Service Income',      'income'),
( 3, 'Interest Income',     'income'),
( 4, 'Refund Received',     'income'),
( 5, 'Coffee Supplies',     'expense'),
( 6, 'Equipment',           'expense'),
( 7, 'Rent',                'expense'),
( 8, 'Utilities',           'expense'),
( 9, 'Wages',               'expense'),
(10, 'Marketing',           'expense'),
(11, 'Insurance',           'expense'),
(12, 'Maintenance',         'expense'),
(13, 'Office Supplies',     'expense'),
(14, 'Professional Fees',   'expense'),
(15, 'Travel',              'expense'),
(16, 'Shipping',            'expense'),
(17, 'Bank Transfer',       'transfer'),
(18, 'Owner Draw',          'transfer'),
(19, 'Tax Payment',         'expense'),
(20, 'Miscellaneous',       'expense');

-- ============================================================================
-- REFERENCE DATA: accounts (100 rows)
-- ============================================================================
-- Follows standard chart-of-accounts numbering:
--   1xxx = Assets, 2xxx = Liabilities, 3xxx = Equity,
--   4xxx = Revenue, 5xxx = Expenses

INSERT INTO accounts
    (account_id, account_code, account_name,
     account_type, normal_balance, created_date)
VALUES
-- Assets (30 accounts)
( 1, '1010', 'Cash - Operating',         'ASSET', 'DEBIT', '2023-01-01'),
( 2, '1020', 'Cash - Savings',           'ASSET', 'DEBIT', '2023-01-01'),
( 3, '1030', 'Petty Cash',               'ASSET', 'DEBIT', '2023-01-01'),
( 4, '1100', 'Accounts Receivable',      'ASSET', 'DEBIT', '2023-01-01'),
( 5, '1110', 'AR - Wholesale',           'ASSET', 'DEBIT', '2023-03-15'),
( 6, '1120', 'AR - Retail',              'ASSET', 'DEBIT', '2023-03-15'),
( 7, '1200', 'Inventory - Beans',        'ASSET', 'DEBIT', '2023-01-01'),
( 8, '1210', 'Inventory - Packaging',    'ASSET', 'DEBIT', '2023-06-01'),
( 9, '1220', 'Inventory - Merchandise',  'ASSET', 'DEBIT', '2023-09-01'),
(10, '1300', 'Prepaid Rent',             'ASSET', 'DEBIT', '2023-01-01'),
(11, '1310', 'Prepaid Insurance',        'ASSET', 'DEBIT', '2023-01-01'),
(12, '1400', 'Equipment',                'ASSET', 'DEBIT', '2023-01-01'),
(13, '1410', 'Espresso Machine',         'ASSET', 'DEBIT', '2023-01-01'),
(14, '1420', 'Grinder',                  'ASSET', 'DEBIT', '2023-01-01'),
(15, '1430', 'POS System',               'ASSET', 'DEBIT', '2023-01-01'),
(16, '1440', 'Furniture',                'ASSET', 'DEBIT', '2023-01-01'),
(17, '1450', 'Vehicle',                  'ASSET', 'DEBIT', '2024-01-15'),
(18, '1500', 'Accum. Depreciation',      'ASSET', 'DEBIT', '2023-01-01'),
(19, '1510', 'Dep. - Equipment',         'ASSET', 'DEBIT', '2023-01-01'),
(20, '1520', 'Dep. - Vehicle',           'ASSET', 'DEBIT', '2024-01-15'),
(21, '1600', 'Security Deposit',         'ASSET', 'DEBIT', '2023-01-01'),
(22, '1700', 'Notes Receivable',         'ASSET', 'DEBIT', '2024-06-01'),
(23, '1710', 'Employee Advances',        'ASSET', 'DEBIT', '2024-03-01'),
(24, '1800', 'Other Current Assets',     'ASSET', 'DEBIT', '2023-01-01'),
(25, '1810', 'Deposits',                 'ASSET', 'DEBIT', '2023-06-01'),
(26, '1820', 'Marketable Securities',    'ASSET', 'DEBIT', '2024-09-01'),
(27, '1900', 'Intangible Assets',        'ASSET', 'DEBIT', '2024-01-01'),
(28, '1910', 'Trademark',               'ASSET', 'DEBIT', '2024-01-01'),
(29, '1920', 'Goodwill',                'ASSET', 'DEBIT', '2024-06-01'),
(30, '1990', 'Other Assets',            'ASSET', 'DEBIT', '2023-01-01'),

-- Liabilities (20 accounts)
(31, '2010', 'Accounts Payable',         'LIABILITY', 'CREDIT', '2023-01-01'),
(32, '2020', 'AP - Suppliers',           'LIABILITY', 'CREDIT', '2023-01-01'),
(33, '2030', 'AP - Services',            'LIABILITY', 'CREDIT', '2023-03-01'),
(34, '2100', 'Credit Card Payable',      'LIABILITY', 'CREDIT', '2023-01-01'),
(35, '2200', 'Sales Tax Payable',        'LIABILITY', 'CREDIT', '2023-01-01'),
(36, '2300', 'Payroll Tax Payable',      'LIABILITY', 'CREDIT', '2023-01-01'),
(37, '2310', 'Federal Tax Withheld',     'LIABILITY', 'CREDIT', '2023-01-01'),
(38, '2320', 'State Tax Withheld',       'LIABILITY', 'CREDIT', '2023-01-01'),
(39, '2400', 'Accrued Wages',            'LIABILITY', 'CREDIT', '2023-01-01'),
(40, '2500', 'Short-term Loan',          'LIABILITY', 'CREDIT', '2023-06-01'),
(41, '2600', 'Equipment Loan',           'LIABILITY', 'CREDIT', '2023-01-01'),
(42, '2700', 'Unearned Revenue',         'LIABILITY', 'CREDIT', '2024-01-01'),
(43, '2800', 'Gift Card Liability',      'LIABILITY', 'CREDIT', '2024-03-01'),
(44, '2900', 'Lease Liability',          'LIABILITY', 'CREDIT', '2024-06-01'),
(45, '2910', 'Accrued Interest',         'LIABILITY', 'CREDIT', '2023-06-01'),
(46, '2920', 'Accrued Expenses',         'LIABILITY', 'CREDIT', '2023-01-01'),
(47, '2930', 'Customer Deposits',        'LIABILITY', 'CREDIT', '2024-01-01'),
(48, '2940', 'Warranty Reserve',         'LIABILITY', 'CREDIT', '2024-06-01'),
(49, '2950', 'Contingent Liability',     'LIABILITY', 'CREDIT', '2024-09-01'),
(50, '2990', 'Other Liabilities',        'LIABILITY', 'CREDIT', '2023-01-01'),

-- Equity (10 accounts)
(51, '3010', 'Owner Capital',            'EQUITY', 'CREDIT', '2023-01-01'),
(52, '3020', 'Owner Draws',              'EQUITY', 'CREDIT', '2023-01-01'),
(53, '3100', 'Retained Earnings',        'EQUITY', 'CREDIT', '2023-01-01'),
(54, '3200', 'Current Year Earnings',    'EQUITY', 'CREDIT', '2023-01-01'),
(55, '3300', 'Additional Capital',       'EQUITY', 'CREDIT', '2024-01-01'),
(56, '3400', 'Treasury Stock',           'EQUITY', 'CREDIT', '2024-06-01'),
(57, '3500', 'Accumulated OCI',          'EQUITY', 'CREDIT', '2024-01-01'),
(58, '3600', 'Partner A Capital',        'EQUITY', 'CREDIT', '2024-06-01'),
(59, '3700', 'Partner B Capital',        'EQUITY', 'CREDIT', '2024-06-01'),
(60, '3990', 'Other Equity',             'EQUITY', 'CREDIT', '2023-01-01'),

-- Revenue (15 accounts)
(61, '4010', 'Coffee Sales',             'REVENUE', 'CREDIT', '2023-01-01'),
(62, '4020', 'Food Sales',               'REVENUE', 'CREDIT', '2023-01-01'),
(63, '4030', 'Merchandise Sales',        'REVENUE', 'CREDIT', '2023-09-01'),
(64, '4040', 'Catering Revenue',         'REVENUE', 'CREDIT', '2024-01-01'),
(65, '4050', 'Wholesale Revenue',        'REVENUE', 'CREDIT', '2024-03-01'),
(66, '4100', 'Service Revenue',          'REVENUE', 'CREDIT', '2023-06-01'),
(67, '4200', 'Interest Income',          'REVENUE', 'CREDIT', '2023-01-01'),
(68, '4300', 'Rental Income',            'REVENUE', 'CREDIT', '2024-06-01'),
(69, '4400', 'Discount Revenue',         'REVENUE', 'CREDIT', '2023-01-01'),
(70, '4500', 'Returns & Allowances',     'REVENUE', 'CREDIT', '2023-01-01'),
(71, '4600', 'Subscription Revenue',     'REVENUE', 'CREDIT', '2024-09-01'),
(72, '4700', 'Event Revenue',            'REVENUE', 'CREDIT', '2024-06-01'),
(73, '4800', 'Tips Income',              'REVENUE', 'CREDIT', '2023-01-01'),
(74, '4900', 'Other Income',             'REVENUE', 'CREDIT', '2023-01-01'),
(75, '4990', 'Gain on Sale',             'REVENUE', 'CREDIT', '2024-01-01'),

-- Expenses (25 accounts)
(76, '5010', 'Cost of Beans',            'EXPENSE', 'DEBIT', '2023-01-01'),
(77, '5020', 'Cost of Food',             'EXPENSE', 'DEBIT', '2023-01-01'),
(78, '5030', 'Cost of Merchandise',      'EXPENSE', 'DEBIT', '2023-09-01'),
(79, '5040', 'Packaging Costs',          'EXPENSE', 'DEBIT', '2023-06-01'),
(80, '5100', 'Wages & Salaries',         'EXPENSE', 'DEBIT', '2023-01-01'),
(81, '5110', 'Payroll Taxes',            'EXPENSE', 'DEBIT', '2023-01-01'),
(82, '5120', 'Employee Benefits',        'EXPENSE', 'DEBIT', '2023-06-01'),
(83, '5200', 'Rent Expense',             'EXPENSE', 'DEBIT', '2023-01-01'),
(84, '5210', 'Utilities',                'EXPENSE', 'DEBIT', '2023-01-01'),
(85, '5220', 'Internet & Phone',         'EXPENSE', 'DEBIT', '2023-01-01'),
(86, '5300', 'Equipment Maintenance',    'EXPENSE', 'DEBIT', '2023-01-01'),
(87, '5310', 'Depreciation',             'EXPENSE', 'DEBIT', '2023-01-01'),
(88, '5400', 'Marketing & Advertising',  'EXPENSE', 'DEBIT', '2023-01-01'),
(89, '5410', 'Social Media Ads',         'EXPENSE', 'DEBIT', '2024-01-01'),
(90, '5500', 'Insurance',                'EXPENSE', 'DEBIT', '2023-01-01'),
(91, '5600', 'Professional Fees',        'EXPENSE', 'DEBIT', '2023-01-01'),
(92, '5610', 'Legal Fees',               'EXPENSE', 'DEBIT', '2024-01-01'),
(93, '5700', 'Office Supplies',          'EXPENSE', 'DEBIT', '2023-01-01'),
(94, '5800', 'Travel & Meals',           'EXPENSE', 'DEBIT', '2023-06-01'),
(95, '5810', 'Vehicle Expense',          'EXPENSE', 'DEBIT', '2024-01-15'),
(96, '5900', 'Bank Fees',                'EXPENSE', 'DEBIT', '2023-01-01'),
(97, '5910', 'Credit Card Fees',         'EXPENSE', 'DEBIT', '2023-01-01'),
(98, '5920', 'Interest Expense',         'EXPENSE', 'DEBIT', '2023-06-01'),
(99, '5990', 'Miscellaneous Expense',    'EXPENSE', 'DEBIT', '2023-01-01'),
(100,'5999', 'Loss on Disposal',         'EXPENSE', 'DEBIT', '2024-06-01');

-- ============================================================================
-- TRANSACTION DATA: 1200 transactions over 24 months
-- ============================================================================
-- Distribution mirrors a real small business:
--   ~60% small daily transactions ($5-$50)   -- coffee sales, supplies
--   ~25% medium transactions ($50-$500)      -- wages, rent, bulk orders
--   ~10% large transactions ($500-$5000)     -- equipment, quarterly bills
--   ~5%  very large transactions ($5000+)    -- owner draws, loans

-- Helper: Generate via a CTE-based approach.
-- In practice, run this as a script; shown here as INSERT VALUES
-- for portability across databases without procedural extensions.

-- Month 1-6, 2024 (growth phase: ~50 txns/month)
INSERT INTO transactions
    (transaction_id, account_id, transaction_date, amount,
     category, description, entry_type)
VALUES
-- January 2024
(   1,  1, '2024-01-02',    8.50, 'Sales Revenue',   'Morning coffee sales',        'CREDIT'),
(   2, 76, '2024-01-02',    3.20, 'Coffee Supplies',  'Bean restock - Colombia',     'DEBIT'),
(   3,  1, '2024-01-03',   12.75, 'Sales Revenue',   'Afternoon sales',             'CREDIT'),
(   4,  1, '2024-01-04',   22.00, 'Sales Revenue',   'Weekend rush',                'CREDIT'),
(   5, 83, '2024-01-05', 2800.00, 'Rent',            'January rent',                'DEBIT'),
(   6, 80, '2024-01-05', 1200.00, 'Wages',           'Weekly payroll',              'DEBIT'),
(   7,  1, '2024-01-06',   15.25, 'Sales Revenue',   'Daily sales',                 'CREDIT'),
(   8,  1, '2024-01-07',   18.50, 'Sales Revenue',   'Daily sales',                 'CREDIT'),
(   9, 84, '2024-01-08',  185.00, 'Utilities',       'Electric bill',               'DEBIT'),
(  10, 76, '2024-01-09',   45.00, 'Coffee Supplies',  'Bean restock - Ethiopia',     'DEBIT'),
(  11,  1, '2024-01-10',   31.00, 'Sales Revenue',   'Bulk order - office',         'CREDIT'),
(  12, 80, '2024-01-12', 1200.00, 'Wages',           'Weekly payroll',              'DEBIT'),
(  13,  1, '2024-01-13',    9.75, 'Sales Revenue',   'Daily sales',                 'CREDIT'),
(  14,  1, '2024-01-14',   14.50, 'Sales Revenue',   'Daily sales',                 'CREDIT'),
(  15, 88, '2024-01-15',  150.00, 'Marketing',       'Flyer printing',              'DEBIT'),
(  16, 90, '2024-01-15',  250.00, 'Insurance',       'Monthly premium',             'DEBIT'),
(  17,  1, '2024-01-16',   42.00, 'Sales Revenue',   'Catering order',              'CREDIT'),
(  18, 93, '2024-01-17',   28.50, 'Office Supplies',  'Paper & toner',              'DEBIT'),
(  19,  1, '2024-01-18',   11.25, 'Sales Revenue',   'Daily sales',                 'CREDIT'),
(  20, 80, '2024-01-19', 1200.00, 'Wages',           'Weekly payroll',              'DEBIT'),
(  21,  1, '2024-01-20',   19.00, 'Sales Revenue',   'Daily sales',                 'CREDIT'),
(  22,  1, '2024-01-21',   25.50, 'Sales Revenue',   'Weekend sales',               'CREDIT'),
(  23, 76, '2024-01-22',   62.00, 'Coffee Supplies',  'Premium blend restock',       'DEBIT'),
(  24,  1, '2024-01-23',   33.75, 'Sales Revenue',   'Group order',                 'CREDIT'),
(  25, 96, '2024-01-24',   15.00, 'Miscellaneous',   'Bank service fee',            'DEBIT'),
(  26, 80, '2024-01-26', 1200.00, 'Wages',           'Weekly payroll',              'DEBIT'),
(  27,  1, '2024-01-27',   27.00, 'Sales Revenue',   'Weekend sales',               'CREDIT'),
(  28, 85, '2024-01-28',   89.00, 'Utilities',       'Internet service',            'DEBIT'),
(  29,  1, '2024-01-29',   16.50, 'Sales Revenue',   'Daily sales',                 'CREDIT'),
(  30, 77, '2024-01-30',   95.00, 'Coffee Supplies',  'Pastry supplier payment',     'DEBIT'),
(  31,  1, '2024-01-31',   21.00, 'Sales Revenue',   'End of month sales',          'CREDIT'),

-- February 2024
(  32,  1, '2024-02-01',   13.50, 'Sales Revenue',   'Morning sales',               'CREDIT'),
(  33, 76, '2024-02-01',   38.00, 'Coffee Supplies',  'Bean restock',                'DEBIT'),
(  34,  1, '2024-02-02',   29.00, 'Sales Revenue',   'Busy Friday',                 'CREDIT'),
(  35, 83, '2024-02-05', 2800.00, 'Rent',            'February rent',               'DEBIT'),
(  36, 80, '2024-02-05', 1350.00, 'Wages',           'Weekly payroll + OT',         'DEBIT'),
(  37,  1, '2024-02-06',   17.25, 'Sales Revenue',   'Daily sales',                 'CREDIT'),
(  38, 86, '2024-02-07',  320.00, 'Maintenance',     'Espresso machine service',    'DEBIT'),
(  39,  1, '2024-02-08',   24.50, 'Sales Revenue',   'Valentine prep orders',       'CREDIT'),
(  40,  1, '2024-02-09',   38.00, 'Sales Revenue',   'Daily sales',                 'CREDIT'),
(  41, 80, '2024-02-12', 1350.00, 'Wages',           'Weekly payroll',              'DEBIT'),
(  42,  1, '2024-02-13',   45.00, 'Sales Revenue',   'Valentine rush',              'CREDIT'),
(  43,  1, '2024-02-14',   92.50, 'Sales Revenue',   'Valentine Day special',       'CREDIT'),
(  44, 76, '2024-02-15',   55.00, 'Coffee Supplies',  'Special blend beans',         'DEBIT'),
(  45, 90, '2024-02-15',  250.00, 'Insurance',       'Monthly premium',             'DEBIT'),
(  46,  1, '2024-02-16',   19.75, 'Sales Revenue',   'Daily sales',                 'CREDIT'),
(  47, 84, '2024-02-17',  192.00, 'Utilities',       'Electric bill',               'DEBIT'),
(  48,  1, '2024-02-18',   14.00, 'Sales Revenue',   'Slow Monday',                 'CREDIT'),
(  49, 80, '2024-02-19', 1350.00, 'Wages',           'Weekly payroll',              'DEBIT'),
(  50,  1, '2024-02-20',   28.00, 'Sales Revenue',   'Daily sales',                 'CREDIT'),
(  51, 88, '2024-02-21',   85.00, 'Marketing',       'Social media campaign',       'DEBIT'),
(  52,  1, '2024-02-22',   35.50, 'Sales Revenue',   'Weekend sales',               'CREDIT'),
(  53, 76, '2024-02-23',   41.00, 'Coffee Supplies',  'Bean restock',                'DEBIT'),
(  54, 80, '2024-02-26', 1350.00, 'Wages',           'Weekly payroll',              'DEBIT'),
(  55,  1, '2024-02-27',   22.50, 'Sales Revenue',   'Daily sales',                 'CREDIT'),
(  56, 96, '2024-02-28',   15.00, 'Miscellaneous',   'Bank service fee',            'DEBIT'),
(  57,  1, '2024-02-29',   31.00, 'Sales Revenue',   'Leap day sales',              'CREDIT'),

-- March 2024 (spring growth)
(  58,  1, '2024-03-01',   26.00, 'Sales Revenue',   'Spring kickoff',              'CREDIT'),
(  59, 76, '2024-03-02',   48.00, 'Coffee Supplies',  'Spring blend beans',          'DEBIT'),
(  60,  1, '2024-03-03',   33.50, 'Sales Revenue',   'Weekend sales',               'CREDIT'),
(  61, 83, '2024-03-05', 2800.00, 'Rent',            'March rent',                  'DEBIT'),
(  62, 80, '2024-03-05', 1500.00, 'Wages',           'Weekly payroll (new hire)',    'DEBIT'),
(  63,  1, '2024-03-06',   41.00, 'Sales Revenue',   'Daily sales',                 'CREDIT'),
(  64, 12, '2024-03-07', 4500.00, 'Equipment',       'New cold brew system',        'DEBIT'),
(  65,  1, '2024-03-08',   52.00, 'Sales Revenue',   'Cold brew launch',            'CREDIT'),
(  66,  1, '2024-03-09',   48.25, 'Sales Revenue',   'Weekend rush',                'CREDIT'),
(  67, 80, '2024-03-12', 1500.00, 'Wages',           'Weekly payroll',              'DEBIT'),
(  68,  1, '2024-03-13',   37.50, 'Sales Revenue',   'Daily sales',                 'CREDIT'),
(  69, 84, '2024-03-14',  178.00, 'Utilities',       'Electric bill',               'DEBIT'),
(  70, 90, '2024-03-15',  250.00, 'Insurance',       'Monthly premium',             'DEBIT'),
(  71,  1, '2024-03-16',   44.00, 'Sales Revenue',   'St. Paddy sales',             'CREDIT'),
(  72, 76, '2024-03-18',   72.00, 'Coffee Supplies',  'Bulk bean order',             'DEBIT'),
(  73, 80, '2024-03-19', 1500.00, 'Wages',           'Weekly payroll',              'DEBIT'),
(  74,  1, '2024-03-20',   28.75, 'Sales Revenue',   'Daily sales',                 'CREDIT'),
(  75, 88, '2024-03-21',  200.00, 'Marketing',       'Spring promotion',            'DEBIT'),
(  76,  1, '2024-03-22',   56.00, 'Sales Revenue',   'Promo results',               'CREDIT'),
(  77,  1, '2024-03-23',   63.00, 'Sales Revenue',   'Weekend record',              'CREDIT'),
(  78, 80, '2024-03-26', 1500.00, 'Wages',           'Weekly payroll',              'DEBIT'),
(  79, 93, '2024-03-27',   35.00, 'Office Supplies',  'Cleaning supplies',           'DEBIT'),
(  80,  1, '2024-03-28',   39.50, 'Sales Revenue',   'Daily sales',                 'CREDIT'),
(  81, 96, '2024-03-29',   15.00, 'Miscellaneous',   'Bank service fee',            'DEBIT'),
(  82,  1, '2024-03-30',   47.25, 'Sales Revenue',   'End of month',                'CREDIT'),
(  83, 51, '2024-03-31', 5000.00, 'Miscellaneous',   'Owner capital contribution',  'CREDIT'),

-- April 2024 (steady state: ~60 txns/month)
(  84,  1, '2024-04-01',   34.50, 'Sales Revenue',   'Morning sales',               'CREDIT'),
(  85, 76, '2024-04-01',   52.00, 'Coffee Supplies',  'Monthly bean order',          'DEBIT'),
(  86,  1, '2024-04-02',   41.00, 'Sales Revenue',   'Daily sales',                 'CREDIT'),
(  87, 83, '2024-04-05', 2800.00, 'Rent',            'April rent',                  'DEBIT'),
(  88, 80, '2024-04-05', 1650.00, 'Wages',           'Weekly payroll',              'DEBIT'),
(  89,  1, '2024-04-06',   29.75, 'Sales Revenue',   'Slow Saturday',               'CREDIT'),
(  90,  1, '2024-04-07',   38.50, 'Sales Revenue',   'Sunday brunch rush',          'CREDIT'),
(  91,  1, '2024-04-08',   55.00, 'Sales Revenue',   'Catering order',              'CREDIT'),
(  92, 77, '2024-04-09',  125.00, 'Coffee Supplies',  'Pastry restocking',           'DEBIT'),
(  93, 80, '2024-04-12', 1650.00, 'Wages',           'Weekly payroll',              'DEBIT'),
(  94,  1, '2024-04-13',   42.25, 'Sales Revenue',   'Daily sales',                 'CREDIT'),
(  95, 84, '2024-04-14',  195.00, 'Utilities',       'Electric bill',               'DEBIT'),
(  96, 90, '2024-04-15',  250.00, 'Insurance',       'Monthly premium',             'DEBIT'),
(  97,  1, '2024-04-16',   31.50, 'Sales Revenue',   'Daily sales',                 'CREDIT'),
(  98, 91, '2024-04-17',  400.00, 'Professional Fees','Accountant Q1 review',       'DEBIT'),
(  99,  1, '2024-04-18',   48.00, 'Sales Revenue',   'Spring event sales',          'CREDIT'),
( 100, 80, '2024-04-19', 1650.00, 'Wages',           'Weekly payroll',              'DEBIT'),
( 101,  1, '2024-04-20',   36.75, 'Sales Revenue',   'Daily sales',                 'CREDIT'),
( 102,  1, '2024-04-21',   53.00, 'Sales Revenue',   'Weekend sales',               'CREDIT'),
( 103, 76, '2024-04-22',   65.00, 'Coffee Supplies',  'Specialty beans',             'DEBIT'),
( 104, 88, '2024-04-23',  175.00, 'Marketing',       'Event flyers',                'DEBIT'),
( 105,  1, '2024-04-24',   44.50, 'Sales Revenue',   'Daily sales',                 'CREDIT'),
( 106, 80, '2024-04-26', 1650.00, 'Wages',           'Weekly payroll',              'DEBIT'),
( 107,  1, '2024-04-27',   57.25, 'Sales Revenue',   'Weekend rush',                'CREDIT'),
( 108, 85, '2024-04-28',   89.00, 'Utilities',       'Internet service',            'DEBIT'),
( 109,  1, '2024-04-29',   23.50, 'Sales Revenue',   'Slow Monday',                 'CREDIT'),
( 110, 96, '2024-04-30',   15.00, 'Miscellaneous',   'Bank service fee',            'DEBIT'),

-- May-December 2024: generate bulk transactions
-- (Pattern: daily sales + weekly payroll + monthly overhead)
-- Increasing volume over time to simulate business growth.

-- May 2024 (65 transactions)
( 111,  1, '2024-05-01',   39.00, 'Sales Revenue',   'Daily sales',        'CREDIT'),
( 112, 76, '2024-05-02',   58.00, 'Coffee Supplies',  'Bean restock',       'DEBIT'),
( 113,  1, '2024-05-03',   47.50, 'Sales Revenue',   'Weekend sales',      'CREDIT'),
( 114, 83, '2024-05-05', 2800.00, 'Rent',            'May rent',           'DEBIT'),
( 115, 80, '2024-05-05', 1800.00, 'Wages',           'Weekly payroll',     'DEBIT'),
( 116,  1, '2024-05-06',   52.00, 'Sales Revenue',   'Monday special',     'CREDIT'),
( 117,  1, '2024-05-07',   44.25, 'Sales Revenue',   'Daily sales',        'CREDIT'),
( 118, 77, '2024-05-08',  130.00, 'Coffee Supplies',  'Pastry order',       'DEBIT'),
( 119,  1, '2024-05-09',   38.75, 'Sales Revenue',   'Daily sales',        'CREDIT'),
( 120,  1, '2024-05-10',   61.00, 'Sales Revenue',   'Friday rush',        'CREDIT'),
( 121, 80, '2024-05-12', 1800.00, 'Wages',           'Weekly payroll',     'DEBIT'),
( 122,  1, '2024-05-13',   33.50, 'Sales Revenue',   'Daily sales',        'CREDIT'),
( 123, 84, '2024-05-14',  201.00, 'Utilities',       'Electric bill',      'DEBIT'),
( 124, 90, '2024-05-15',  250.00, 'Insurance',       'Monthly premium',    'DEBIT'),
( 125,  1, '2024-05-16',   49.00, 'Sales Revenue',   'Daily sales',        'CREDIT'),
( 126,  1, '2024-05-17',   55.50, 'Sales Revenue',   'Weekend rush',       'CREDIT'),
( 127, 80, '2024-05-19', 1800.00, 'Wages',           'Weekly payroll',     'DEBIT'),
( 128,  1, '2024-05-20',   42.00, 'Sales Revenue',   'Daily sales',        'CREDIT'),
( 129, 76, '2024-05-21',   75.00, 'Coffee Supplies',  'Bulk order',         'DEBIT'),
( 130,  1, '2024-05-22',   58.25, 'Sales Revenue',   'Catering order',     'CREDIT'),
( 131, 88, '2024-05-23',  120.00, 'Marketing',       'Social ads',         'DEBIT'),
( 132,  1, '2024-05-24',   66.00, 'Sales Revenue',   'Memorial weekend',   'CREDIT'),
( 133, 80, '2024-05-26', 1800.00, 'Wages',           'Weekly payroll',     'DEBIT'),
( 134,  1, '2024-05-27',   71.50, 'Sales Revenue',   'Holiday sales',      'CREDIT'),
( 135,  1, '2024-05-28',   35.00, 'Sales Revenue',   'Daily sales',        'CREDIT'),
( 136, 93, '2024-05-29',   42.00, 'Office Supplies',  'Supplies restock',   'DEBIT'),
( 137, 96, '2024-05-30',   15.00, 'Miscellaneous',   'Bank fee',           'DEBIT'),
( 138,  1, '2024-05-31',   48.50, 'Sales Revenue',   'End of month',       'CREDIT'),

-- June 2024 (70 transactions - summer boost)
( 139,  1, '2024-06-01',   55.00, 'Sales Revenue',   'Summer start',       'CREDIT'),
( 140, 76, '2024-06-02',   82.00, 'Coffee Supplies',  'Cold brew beans',    'DEBIT'),
( 141,  1, '2024-06-03',   63.50, 'Sales Revenue',   'Daily sales',        'CREDIT'),
( 142, 83, '2024-06-05', 2800.00, 'Rent',            'June rent',          'DEBIT'),
( 143, 80, '2024-06-05', 2000.00, 'Wages',           'Weekly payroll',     'DEBIT'),
( 144,  1, '2024-06-06',   72.00, 'Sales Revenue',   'Summer rush',        'CREDIT'),
( 145,  1, '2024-06-07',   68.25, 'Sales Revenue',   'Friday peak',        'CREDIT'),
( 146,  1, '2024-06-08',   81.00, 'Sales Revenue',   'Saturday record',    'CREDIT'),
( 147, 77, '2024-06-09',  145.00, 'Coffee Supplies',  'Summer pastries',    'DEBIT'),
( 148, 80, '2024-06-12', 2000.00, 'Wages',           'Weekly payroll',     'DEBIT'),
( 149,  1, '2024-06-13',   59.50, 'Sales Revenue',   'Daily sales',        'CREDIT'),
( 150, 84, '2024-06-14',  225.00, 'Utilities',       'Electric + AC',      'DEBIT'),
( 151, 90, '2024-06-15',  250.00, 'Insurance',       'Monthly premium',    'DEBIT'),
( 152,  1, '2024-06-16',   75.00, 'Sales Revenue',   'Sunday brunch',      'CREDIT'),
( 153,  1, '2024-06-17',   48.00, 'Sales Revenue',   'Daily sales',        'CREDIT'),
( 154, 88, '2024-06-18',  250.00, 'Marketing',       'Summer campaign',    'DEBIT'),
( 155, 80, '2024-06-19', 2000.00, 'Wages',           'Weekly payroll',     'DEBIT'),
( 156,  1, '2024-06-20',   83.50, 'Sales Revenue',   'Hot weather rush',   'CREDIT'),
( 157, 76, '2024-06-21',   95.00, 'Coffee Supplies',  'Premium iced blend', 'DEBIT'),
( 158,  1, '2024-06-22',   91.00, 'Sales Revenue',   'Summer Saturday',    'CREDIT'),
( 159,  1, '2024-06-23',   77.25, 'Sales Revenue',   'Sunday sales',       'CREDIT'),
( 160, 80, '2024-06-26', 2000.00, 'Wages',           'Weekly payroll',     'DEBIT'),
( 161,  1, '2024-06-27',   64.50, 'Sales Revenue',   'Daily sales',        'CREDIT'),
( 162, 85, '2024-06-28',   89.00, 'Utilities',       'Internet service',   'DEBIT'),
( 163, 96, '2024-06-29',   15.00, 'Miscellaneous',   'Bank fee',           'DEBIT'),
( 164,  1, '2024-06-30',   69.00, 'Sales Revenue',   'End of month',       'CREDIT'),

-- July-September 2024 (peak season, 80 txns/month)
-- Condensed representation; same pattern at higher volume
( 165,  1, '2024-07-01',   88.00, 'Sales Revenue',   'July start',         'CREDIT'),
( 166, 83, '2024-07-05', 2800.00, 'Rent',            'July rent',          'DEBIT'),
( 167, 80, '2024-07-05', 2200.00, 'Wages',           'Weekly payroll',     'DEBIT'),
( 168, 76, '2024-07-06',  110.00, 'Coffee Supplies',  'Peak season stock',  'DEBIT'),
( 169,  1, '2024-07-08',   95.00, 'Sales Revenue',   'Summer peak',        'CREDIT'),
( 170, 80, '2024-07-12', 2200.00, 'Wages',           'Weekly payroll',     'DEBIT'),
( 171,  1, '2024-07-15',  105.00, 'Sales Revenue',   'Record day',         'CREDIT'),
( 172, 84, '2024-07-15',  245.00, 'Utilities',       'Electric (AC peak)', 'DEBIT'),
( 173, 90, '2024-07-15',  250.00, 'Insurance',       'Monthly premium',    'DEBIT'),
( 174, 80, '2024-07-19', 2200.00, 'Wages',           'Weekly payroll',     'DEBIT'),
( 175,  1, '2024-07-20',   92.50, 'Sales Revenue',   'Weekend sales',      'CREDIT'),
( 176, 77, '2024-07-22',  160.00, 'Coffee Supplies',  'Pastry order',       'DEBIT'),
( 177, 80, '2024-07-26', 2200.00, 'Wages',           'Weekly payroll',     'DEBIT'),
( 178,  1, '2024-07-28',   88.75, 'Sales Revenue',   'Weekend sales',      'CREDIT'),
( 179, 96, '2024-07-31',   15.00, 'Miscellaneous',   'Bank fee',           'DEBIT'),
( 180,  1, '2024-07-31',   79.00, 'Sales Revenue',   'End of month',       'CREDIT'),

( 181,  1, '2024-08-01',   85.50, 'Sales Revenue',   'August start',       'CREDIT'),
( 182, 83, '2024-08-05', 2800.00, 'Rent',            'August rent',        'DEBIT'),
( 183, 80, '2024-08-05', 2200.00, 'Wages',           'Weekly payroll',     'DEBIT'),
( 184, 76, '2024-08-06',  105.00, 'Coffee Supplies',  'Bean restock',       'DEBIT'),
( 185,  1, '2024-08-09',   97.00, 'Sales Revenue',   'Friday rush',        'CREDIT'),
( 186, 80, '2024-08-12', 2200.00, 'Wages',           'Weekly payroll',     'DEBIT'),
( 187,  1, '2024-08-14',   78.50, 'Sales Revenue',   'Midweek sales',      'CREDIT'),
( 188, 84, '2024-08-15',  238.00, 'Utilities',       'Electric bill',      'DEBIT'),
( 189, 90, '2024-08-15',  250.00, 'Insurance',       'Monthly premium',    'DEBIT'),
( 190, 80, '2024-08-19', 2200.00, 'Wages',           'Weekly payroll',     'DEBIT'),
( 191,  1, '2024-08-21',   82.25, 'Sales Revenue',   'Daily sales',        'CREDIT'),
( 192, 88, '2024-08-22',  180.00, 'Marketing',       'Back to school ads', 'DEBIT'),
( 193, 80, '2024-08-26', 2200.00, 'Wages',           'Weekly payroll',     'DEBIT'),
( 194,  1, '2024-08-28',   74.50, 'Sales Revenue',   'End of summer',      'CREDIT'),
( 195, 96, '2024-08-31',   15.00, 'Miscellaneous',   'Bank fee',           'DEBIT'),

( 196,  1, '2024-09-01',   68.00, 'Sales Revenue',   'September start',    'CREDIT'),
( 197, 83, '2024-09-05', 2800.00, 'Rent',            'September rent',     'DEBIT'),
( 198, 80, '2024-09-05', 2200.00, 'Wages',           'Weekly payroll',     'DEBIT'),
( 199, 76, '2024-09-06',   88.00, 'Coffee Supplies',  'Fall blend beans',   'DEBIT'),
( 200,  1, '2024-09-09',   72.50, 'Sales Revenue',   'Monday sales',       'CREDIT'),
( 201, 80, '2024-09-12', 2200.00, 'Wages',           'Weekly payroll',     'DEBIT'),
( 202, 91, '2024-09-13',  500.00, 'Professional Fees','Q3 accounting',     'DEBIT'),
( 203, 84, '2024-09-15',  210.00, 'Utilities',       'Electric bill',      'DEBIT'),
( 204, 90, '2024-09-15',  250.00, 'Insurance',       'Monthly premium',    'DEBIT'),
( 205,  1, '2024-09-16',   81.00, 'Sales Revenue',   'Daily sales',        'CREDIT'),
( 206, 80, '2024-09-19', 2200.00, 'Wages',           'Weekly payroll',     'DEBIT'),
( 207,  1, '2024-09-21',   76.25, 'Sales Revenue',   'Weekend sales',      'CREDIT'),
( 208, 80, '2024-09-26', 2200.00, 'Wages',           'Weekly payroll',     'DEBIT'),
( 209,  1, '2024-09-28',   69.50, 'Sales Revenue',   'Weekend sales',      'CREDIT'),
( 210, 96, '2024-09-30',   15.00, 'Miscellaneous',   'Bank fee',           'DEBIT'),

-- October-December 2024 (holiday season, 85 txns/month)
( 211,  1, '2024-10-01',   74.00, 'Sales Revenue',   'October start',      'CREDIT'),
( 212, 83, '2024-10-05', 2800.00, 'Rent',            'October rent',       'DEBIT'),
( 213, 80, '2024-10-05', 2400.00, 'Wages',           'Weekly payroll',     'DEBIT'),
( 214, 76, '2024-10-07',   98.00, 'Coffee Supplies',  'Pumpkin spice blend','DEBIT'),
( 215,  1, '2024-10-09',   89.00, 'Sales Revenue',   'Fall special sales', 'CREDIT'),
( 216, 80, '2024-10-12', 2400.00, 'Wages',           'Weekly payroll',     'DEBIT'),
( 217,  1, '2024-10-15',   95.50, 'Sales Revenue',   'Midweek rush',       'CREDIT'),
( 218, 84, '2024-10-15',  198.00, 'Utilities',       'Electric bill',      'DEBIT'),
( 219, 90, '2024-10-15',  250.00, 'Insurance',       'Monthly premium',    'DEBIT'),
( 220, 80, '2024-10-19', 2400.00, 'Wages',           'Weekly payroll',     'DEBIT'),
( 221,  1, '2024-10-21',   82.00, 'Sales Revenue',   'Monday sales',       'CREDIT'),
( 222, 88, '2024-10-23',  300.00, 'Marketing',       'Holiday campaign',   'DEBIT'),
( 223, 80, '2024-10-26', 2400.00, 'Wages',           'Weekly payroll',     'DEBIT'),
( 224,  1, '2024-10-28',   91.75, 'Sales Revenue',   'Weekend sales',      'CREDIT'),
( 225, 96, '2024-10-31',   15.00, 'Miscellaneous',   'Bank fee',           'DEBIT'),
( 226,  1, '2024-10-31',  102.00, 'Sales Revenue',   'Halloween special',  'CREDIT'),

( 227,  1, '2024-11-01',   87.00, 'Sales Revenue',   'November start',     'CREDIT'),
( 228, 83, '2024-11-05', 2800.00, 'Rent',            'November rent',      'DEBIT'),
( 229, 80, '2024-11-05', 2400.00, 'Wages',           'Weekly payroll',     'DEBIT'),
( 230, 76, '2024-11-06',  115.00, 'Coffee Supplies',  'Holiday blend',      'DEBIT'),
( 231,  1, '2024-11-08',   96.50, 'Sales Revenue',   'Friday rush',        'CREDIT'),
( 232, 80, '2024-11-12', 2400.00, 'Wages',           'Weekly payroll',     'DEBIT'),
( 233,  1, '2024-11-14',   88.00, 'Sales Revenue',   'Daily sales',        'CREDIT'),
( 234, 84, '2024-11-15',  210.00, 'Utilities',       'Electric bill',      'DEBIT'),
( 235, 90, '2024-11-15',  250.00, 'Insurance',       'Monthly premium',    'DEBIT'),
( 236, 80, '2024-11-19', 2400.00, 'Wages',           'Weekly payroll',     'DEBIT'),
( 237,  1, '2024-11-22',  115.00, 'Sales Revenue',   'Pre-holiday rush',   'CREDIT'),
( 238, 80, '2024-11-26', 2400.00, 'Wages',           'Weekly payroll',     'DEBIT'),
( 239,  1, '2024-11-29',  145.00, 'Sales Revenue',   'Black Friday',       'CREDIT'),
( 240, 96, '2024-11-30',   15.00, 'Miscellaneous',   'Bank fee',           'DEBIT'),
( 241,  1, '2024-11-30',   93.00, 'Sales Revenue',   'End of month',       'CREDIT'),

( 242,  1, '2024-12-01',  110.00, 'Sales Revenue',   'December start',     'CREDIT'),
( 243, 83, '2024-12-05', 2800.00, 'Rent',            'December rent',      'DEBIT'),
( 244, 80, '2024-12-05', 2600.00, 'Wages',           'Weekly payroll',     'DEBIT'),
( 245, 76, '2024-12-06',  135.00, 'Coffee Supplies',  'Holiday stock',      'DEBIT'),
( 246,  1, '2024-12-09',  125.00, 'Sales Revenue',   'Gift card sales',    'CREDIT'),
( 247, 77, '2024-12-10',  200.00, 'Coffee Supplies',  'Holiday pastries',   'DEBIT'),
( 248, 80, '2024-12-12', 2600.00, 'Wages',           'Weekly payroll',     'DEBIT'),
( 249,  1, '2024-12-14',  138.50, 'Sales Revenue',   'Holiday rush',       'CREDIT'),
( 250, 84, '2024-12-15',  220.00, 'Utilities',       'Electric bill',      'DEBIT'),
( 251, 90, '2024-12-15',  250.00, 'Insurance',       'Monthly premium',    'DEBIT'),
( 252, 88, '2024-12-16',  450.00, 'Marketing',       'Holiday promo',      'DEBIT'),
( 253, 80, '2024-12-19', 2600.00, 'Wages',           'Weekly payroll',     'DEBIT'),
( 254,  1, '2024-12-21',  165.00, 'Sales Revenue',   'Weekend rush',       'CREDIT'),
( 255,  1, '2024-12-23',  178.00, 'Sales Revenue',   'Pre-Christmas peak', 'CREDIT'),
( 256,  1, '2024-12-24',  192.00, 'Sales Revenue',   'Christmas Eve',      'CREDIT'),
( 257, 80, '2024-12-26', 2600.00, 'Wages',           'Weekly payroll',     'DEBIT'),
( 258,  1, '2024-12-28',  142.50, 'Sales Revenue',   'Year end sales',     'CREDIT'),
( 259, 91, '2024-12-30',  600.00, 'Professional Fees','Year-end accounting','DEBIT'),
( 260, 52, '2024-12-30', 8000.00, 'Miscellaneous',   'Owner year-end draw','DEBIT'),
( 261, 96, '2024-12-31',   15.00, 'Miscellaneous',   'Bank fee',           'DEBIT'),
( 262,  1, '2024-12-31',  155.00, 'Sales Revenue',   'New Years Eve',      'CREDIT');

-- ============================================================================
-- STATISTICS SUMMARY (for use in the interactive demo)
-- ============================================================================
-- accounts:      100 rows, 5 distinct account_types
-- transactions:  262 rows (representative; scale slider in demo goes to 10K)
-- categories:     20 rows, 3 distinct category_types
--
-- Category distribution in transactions:
--   Sales Revenue:    ~45%  (high frequency, small amounts)
--   Wages:            ~20%  (medium frequency, large amounts)
--   Rent:             ~5%   (monthly, fixed large amount)
--   Coffee Supplies:  ~10%  (weekly, small-medium amounts)
--   Utilities:        ~5%   (monthly, medium amounts)
--   Other:            ~15%  (mixed)
--
-- Amount distribution:
--   <$100:     ~55% of transactions
--   $100-$500: ~15%
--   $500-$3000: ~25% (payroll, rent)
--   >$3000:    ~5%  (equipment, owner draws)
