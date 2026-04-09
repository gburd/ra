# Parser Test Suite

This directory contains comprehensive unit tests for all database query plan parsers.

## Test Files

- `postgresParser.test.ts` - PostgreSQL EXPLAIN JSON parser tests (21 tests)
- `mysqlParser.test.ts` - MySQL EXPLAIN FORMAT=JSON parser tests (20 tests)
- `sqliteParser.test.ts` - SQLite EXPLAIN QUERY PLAN parser tests (34 tests)
- `duckdbParser.test.ts` - DuckDB EXPLAIN parser tests (37 tests)

## Running Tests

```bash
# Run all parser tests
npm test src/parsers/__tests__

# Run specific parser
npm test src/parsers/__tests__/postgresParser.test.ts

# Watch mode
npm run test:watch src/parsers/__tests__

# With UI
npm run test:ui
```

## Test Structure

Each test file follows this pattern:

```typescript
describe('parserName', () => {
  beforeEach(() => {
    // Reset state
  });

  describe('parseFunction', () => {
    // Valid input tests
  });

  describe('error handling', () => {
    // Invalid/malformed input tests
  });

  describe('node extraction', () => {
    // Node parsing verification
  });

  describe('edge extraction', () => {
    // Edge/relationship tests
  });

  describe('cost extraction', () => {
    // Cost parsing tests
  });

  describe('realistic examples', () => {
    // Real-world query plan tests
  });
});
```

## Coverage Areas

### Valid Input Parsing
- Simple plans
- Nested/hierarchical plans
- Multiple children
- Format variations

### Error Handling
- Invalid JSON
- Empty strings
- Null/undefined inputs
- Malformed structures

### Data Extraction
- Operation names
- Relation names
- Row estimates
- Cost values
- Metadata

### Edge Cases
- Single-node plans
- Deep nesting
- Missing optional data
- Zero/large values

## Sample Fixtures

### PostgreSQL
```typescript
const rawPlan = JSON.stringify({
  Plan: {
    'Node Type': 'Seq Scan',
    'Relation Name': 'employees',
    'Startup Cost': 0.0,
    'Total Cost': 35.5,
    'Plan Rows': 2550,
  },
});
```

### MySQL
```typescript
const rawPlan = JSON.stringify({
  query_block: {
    select_id: 1,
    cost_info: {
      query_cost: '1005.00',
    },
    table: {
      table_name: 'employees',
      access_type: 'ALL',
      rows_examined_per_scan: 1000,
    },
  },
});
```

### SQLite
```typescript
const rawPlan = `
0 0 0 SCAN TABLE employees
1 0 0 SEARCH TABLE departments USING INDEX idx_dept_id
`.trim();
```

### DuckDB
```typescript
const rawPlan = `
SEQ_SCAN [employees] 1000 Rows
INDEX_SCAN [departments] 100 Rows
`.trim();
```

## Test Results

```
Test Files  4 passed (4)
Tests       112 passed (112)
Duration    ~2-3 seconds
```

## Adding New Tests

When adding tests:

1. Follow the existing structure
2. Test both valid and invalid inputs
3. Verify node and edge counts
4. Check data extraction accuracy
5. Include realistic examples
6. Add error handling tests

## Related Files

- Parser implementations: `src/parsers/*.ts`
- Type definitions: `src/types.ts`
- Parser index: `src/parsers/index.ts`
