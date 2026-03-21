# SQL Query Test Suite from Database Books

This directory contains a comprehensive SQL query test suite derived from major database textbooks and references.

## Purpose

Test Ra's SQL parser and optimizer coverage against real-world query patterns documented in authoritative database books.

## Books Referenced

1. **SQL Performance Explained** - Markus Winand
2. **High Performance MySQL** - Baron Schwartz et al.
3. **PostgreSQL: Up and Running** - Regina Obe
4. **SQL Cookbook** - Anthony Molinaro
5. **Database System Concepts** - Silberschatz, Korth, Sudarshan
6. **SQL Antipatterns** - Bill Karwin
7. **Designing Data-Intensive Applications** - Martin Kleppmann
8. **SQL Queries for Mere Mortals** - John Viescas
9. **T-SQL Fundamentals** - Itzik Ben-Gan
10. **Learning SQL** - Alan Beaulieu

## Test Files

| File | Description | Query Count |
|------|-------------|-------------|
| `01-simple-queries.sql` | Basic SELECT/WHERE/ORDER BY | 20 |
| `02-joins.sql` | All JOIN types | 13 |
| `03-aggregations.sql` | GROUP BY, HAVING, ROLLUP, CUBE | 17 |
| `04-subqueries.sql` | Correlated, uncorrelated, EXISTS | 16 |
| `05-window-functions.sql` | ROW_NUMBER, RANK, LAG, LEAD | 20 |
| `06-ctes.sql` | WITH clauses, recursive CTEs | 13 |
| `07-set-operations.sql` | UNION, INTERSECT, EXCEPT | 15 |
| `08-complex-analytical.sql` | Running totals, cohorts, funnels | 13 |
| `09-sql-cookbook-patterns.sql` | Practical patterns from SQL Cookbook | 14 |
| `10-performance-patterns.sql` | Index-friendly optimizations | 19 |
| `11-antipatterns.sql` | Common mistakes (for validation) | 21 |

**Total**: 181 queries

## Running Tests

### Python Test Script

```bash
python3 tests/book-queries/test-queries.py
```

This validates SQL syntax for all queries and generates:
- `results/RESULTS.md` - Summary statistics
- `results/FAILURES.md` - Detailed failure analysis

### Shell Test Runner (when ra-cli is fixed)

```bash
./tests/book-queries/test-runner.sh
```

This tests both parsing and optimization against ra-cli.

## Current Results

**Success Rate**: 99.45% (180/181 queries)
**Known Issues**: 1 query (parenthesized set operations)

See [docs/testing/book-query-coverage.md](../../docs/testing/book-query-coverage.md) for detailed analysis.

## Query Categories

- **Simple queries**: 61 (33.7%)
- **Window functions**: 28 (15.5%)
- **Aggregations**: 25 (13.8%)
- **CTEs**: 22 (12.2%)
- **Set operations**: 16 (8.8%)
- **Subqueries**: 14 (7.7%)
- **Joins**: 11 (6.1%)
- **Recursive CTEs**: 4 (2.2%)

## Adding New Queries

1. Create or edit a `.sql` file in this directory
2. Add queries separated by semicolons
3. Include comments describing the source book and pattern
4. Run the test script to validate
5. Update coverage documentation

## Query Format

```sql
-- Description of the query pattern
-- Source: "Book Title" by Author, Chapter/Section
SELECT ...
FROM ...;

-- Next query
SELECT ...;
```

## Integration with CI

These tests should be run as part of the CI pipeline to ensure SQL coverage doesn't regress.

## See Also

- [Book Query Coverage Report](../../docs/testing/book-query-coverage.md)
- [Ra Parser Documentation](../../crates/ra-parser/README.md)
