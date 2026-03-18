# Test Case Format

Rule files (`.rra`) embed test cases as SQL code blocks under the
`## Test Cases` heading. The test runner discovers these blocks,
extracts structured expectations from inline comments, and runs each
test through the parse-optimize-compare pipeline.

## Supported comment markers

| Marker | Meaning |
|--------|---------|
| `-- Positive: <desc>` | Starts a positive test (optimization should apply) |
| `-- Negative: <desc>` | Starts a negative test (optimization should NOT apply) |
| `-- Before` | Marks the input SQL in a before/after pair |
| `-- After` | Marks the expected output (only the Before SQL is tested) |
| `-- Input` | Marks the input SQL section |
| `-- Expected: <text>` | Freeform expected outcome description |
| `-- Expected-Rule: <id>` | Expects a specific rule to be applied |

## Test expectations

Each test case maps to one of these expectations:

- **PlanChanged** -- the optimizer must produce a different plan than
  the input (positive test default).
- **PlanUnchanged** -- the optimizer must leave the plan unchanged
  (negative test default).
- **Parses** -- the SQL must parse successfully (fallback when no
  markers are present).
- **RuleApplied** -- a specific rule ID should fire (currently
  approximated by checking if the plan changed).
- **Described** -- a freeform expectation string from `-- Expected:`.

## Example: before/after

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

The runner parses only the "Before" SQL, optimizes it, and checks that
the plan changed.

## Example: positive/negative markers

```sql
-- Positive: basic filter pushdown to left side
SELECT * FROM orders o
JOIN customers c ON o.customer_id = c.id
WHERE o.amount > 1000;

-- Negative: predicate references both sides, cannot push
SELECT * FROM orders o
JOIN customers c ON o.customer_id = c.id
WHERE o.amount > c.credit_limit;
```

Multiple test cases can appear in one SQL block. Each `-- Positive:` or
`-- Negative:` marker starts a new test case.

## Running tests

```sh
# Run all tests
ra-cli test rules/

# Run tests for a single file
ra-cli test rules/logical/predicate-pushdown/filter-through-join.rra

# Run only tests matching a substring
ra-cli test rules/ --filter filter-pushdown

# Verbose output (show passing tests)
ra-cli -v test rules/
```

## Output

```
Running tests from 450 file(s)...

  [PASS] logical/predicate-pushdown/filter-through-join.rra::basic filter pushdown (2ms)
  [FAIL] join-reorder/complex.rra::three-way join
        expected plan to change, but it stayed the same

Test Results: 1020/1794 passed (56.9%)
  Failed: 514 tests
  Skipped: 222 tests
  Errors: 38 tests
  Duration: 34.9s
```

Exit code is non-zero when any test fails.
