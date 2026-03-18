# Test Format for `.rra` Rule Files

This document describes the test case format used in `.rra` rule files and how the test executor processes them.

## Overview

Each `.rra` rule file can contain one or more SQL test blocks in fenced code blocks tagged with `sql` or `test`. The test executor parses these blocks, extracts structured test cases, runs them through the SQL parser and optimizer, and reports pass/fail results.

## Running Tests

```bash
# Run all tests
ra-cli test rules/

# Run tests for a single file
ra-cli test rules/logical/predicate-pushdown/filter-through-join.rra

# Filter by rule ID substring
ra-cli test rules/ --filter filter-through

# Verbose output (shows passing tests and failure details)
ra-cli test rules/ --verbose

# Quiet mode (only shows the summary line)
ra-cli --quiet test rules/
```

## Test Annotations

Test cases are identified by comment annotations in SQL code blocks. Each annotation starts a new test case.

### Positive Tests

Mark tests where the optimizer **should** change the plan:

```sql
-- Positive: basic filter pushdown
SELECT * FROM orders WHERE amount > 100;
```

Expectation: the optimized plan differs from the original.

### Negative Tests

Mark tests where the optimizer should **not** change the plan:

```sql
-- Negative: cannot push through outer join
SELECT * FROM a LEFT JOIN b ON a.id = b.id WHERE b.x = 1;
```

Expectation: the optimized plan is identical to the original.

### Before/After Pairs

Specify both the input and expected output:

```sql
-- Before
SELECT * FROM orders o
JOIN customers c ON o.customer_id = c.id
WHERE o.amount > 1000;

-- After
SELECT * FROM (
    SELECT * FROM orders WHERE amount > 1000
) o
JOIN customers c ON o.customer_id = c.id;
```

Expectation: the optimized plan for the "Before" SQL matches the optimized plan for the "After" SQL.

### Expected Annotations

Describe what the optimizer should do (treated as a positive test):

```sql
-- Expected: uses hardware-accelerated operator
SELECT * FROM sensors WHERE reading > 42.0;
```

### Expected-Rule Annotations

Assert that a specific rule was applied:

```sql
-- Expected-Rule: filter-through-join
SELECT * FROM orders o JOIN items i ON o.id = i.oid
WHERE o.status = 'shipped';
```

Note: since the e-graph optimizer applies rules via equality saturation, the executor approximates this by checking whether the plan changed.

### Bare SQL (No Annotations)

If a code block contains SQL with no structured annotations, it is treated as a "no error" test -- the SQL should parse and optimize without crashing:

```sql
SELECT * FROM users WHERE age > 18;
```

## Test Outcomes

| Outcome | Meaning |
|---------|---------|
| PASS | Test expectation was met |
| FAIL | Test expectation was not met |
| SKIP | SQL could not be parsed (unsupported feature) |
| ERROR | Internal error during optimization |

## Test Statistics

The summary line reports:
- Total test cases
- Passed / Failed / Skipped / Errors
- Pass rate (passed / (passed + failed), excluding skips)

## Multiple Tests Per Block

A single code block can contain multiple test cases separated by annotations:

```sql
-- Positive: simple filter
SELECT * FROM t WHERE x > 1;

-- Negative: non-deterministic
SELECT * FROM t WHERE random() > 0.5;
```

This produces two test cases from one code block.

## Supported SQL Features

The SQL parser currently supports:
- SELECT with column lists and aliases
- FROM with single tables and INNER JOINs
- WHERE with comparison, AND, OR, NOT, IS NULL, IS NOT NULL
- GROUP BY with COUNT, SUM, AVG, MIN, MAX
- Table aliases

Not yet supported (tests using these will be skipped):
- LEFT/RIGHT/FULL OUTER JOINs
- Subqueries
- WITH/CTE
- DISTINCT
- ORDER BY, LIMIT/OFFSET
- HAVING
- UNION/INTERSECT/EXCEPT
- Window functions
