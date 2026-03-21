# Rule: Codegen Disabled Fallback (Impala)

**Category:** database-specific/impala
**File:** `rules/database-specific/impala/codegen-disabled-fallback.rra`

## Metadata

- **ID:** `impala-codegen-disabled-fallback`
- **Version:** "1.0.0"
- **Databases:** impala
- **Tags:** database-specific
- **Authors:** "RA Contributors"


# Codegen Disabled Fallback (Impala)

## Metadata
- **Rule ID**: `impala-codegen-fallback`
- **Category**: Database-Specific / Impala
- **Source**: Apache Impala

## Description

Impala adaptively disables codegen for simple queries where interpretation overhead is lower than compilation overhead.

## Test Cases

### Test 1: Simple query (no codegen)
```sql
SELECT * FROM users WHERE id = 123;

-- Single-row lookup: interpretation faster
-- Codegen disabled automatically
```

### Test 2: Complex query (codegen enabled)
```sql
SELECT user_id, COUNT(*), SUM(amount), AVG(price)
FROM transactions
WHERE date >= '2024-01-01'
GROUP BY user_id
HAVING SUM(amount) > 1000;

-- Complex operators: codegen beneficial
-- LLVM compilation enabled
```

## References
1. **Impala Docs**: "Runtime Code Generation"

## Tags
`database-specific`, `impala`, `codegen`, `adaptive`, `llvm`, `interpretation`
