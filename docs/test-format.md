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
# Run all tests (1199 rule files, ~2600 test cases)
ra-cli test rules/

# Run tests for a single file
ra-cli test rules/logical/predicate-pushdown/filter-through-join.rra

# Run only tests matching a substring
ra-cli test rules/ --filter filter-pushdown

# Verbose output (show passing tests and per-test timing)
ra-cli test rules/ --verbose

# Quiet mode (only show summary)
ra-cli test rules/ --quiet
```

## Output

```
Running tests from 1199 file(s)...

  [FAIL] logical/join-reordering/join-commutativity.rra (1/2 passed)
        - join-commutativity::block_0 (expected plan to change, but it stayed the same)

Summary: 1454/2593 passed (56.1%)
  Failed: 770 tests
  Skipped: 357 tests
  Errors: 12 tests
  Duration: 57.7s
```

Exit code is non-zero when any test fails. Skipped tests (SQL parse
limitations) count as passing for file-level summaries since they
indicate the test infrastructure cannot yet validate that case, not
that the rule is broken.

## Failure categories

Most failures fall into predictable categories:

| Category | Cause |
|----------|-------|
| "plan stayed the same" | Rule not yet implemented in the optimizer |
| "multiple statements not supported" | Test SQL uses semicolons to separate statements |
| "plan should not have changed" | Negative test where the optimizer applies a different rule |
| "unsupported SQL feature" | Parser doesn't support CTEs, INTERVAL, etc. |

## Optimizer configuration

The test runner uses a lightweight optimizer configuration to keep
execution fast:

- **Node limit**: 5,000 (vs 100,000 default)
- **Iteration limit**: 2 (vs 30 default)
- **Time limit**: 1 second per test (vs 10 default)

This is sufficient to detect whether a rule fires without fully
saturating the e-graph.
