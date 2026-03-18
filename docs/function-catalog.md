# Function Catalog

RA catalogs 200+ SQL functions with optimizer-relevant metadata across PostgreSQL, MySQL, SQLite, SQL Server, and Oracle.

## Architecture

The function catalog lives in `crates/ra-catalog/`:

- `src/functions.rs` - Types and built-in function registration
- `src/lib.rs` - Public API and documentation

## Function Categories

| Category | Count | Examples |
|----------|-------|---------|
| Scalar | ~140 | ABS, UPPER, NOW, CAST |
| Aggregate | ~36 | COUNT, SUM, AVG, STDDEV |
| Window | 11 | ROW_NUMBER, RANK, LAG |
| TableValued | ~15 | UNNEST, JSON_TABLE, GENERATE_SERIES |

## Function Families

### Math (35 functions)
ABS, CEIL, FLOOR, ROUND, TRUNC, SQRT, CBRT, POWER, EXP, LN, LOG, LOG2, LOG10, MOD, SIGN, PI, DEGREES, RADIANS, SIN, COS, TAN, ASIN, ACOS, ATAN, ATAN2, COT, GREATEST, LEAST, RANDOM, DIV, GCD, LCM, WIDTH_BUCKET, CEILING, TRUNCATE, POW

### String (42 functions)
UPPER, LOWER, LENGTH, TRIM, SUBSTRING, CONCAT, REPLACE, POSITION, REVERSE, REPEAT, ASCII, CHR, MD5, SHA256, REGEXP_REPLACE, REGEXP_MATCHES, LIKE, ILIKE, SPLIT_PART, FORMAT, and more

### Date/Time (37 functions)
NOW, CURRENT_TIMESTAMP, DATE_TRUNC, EXTRACT, DATE_ADD, DATEDIFF, AGE, TO_CHAR, TO_TIMESTAMP, TIMEZONE, YEAR, MONTH, DAY, HOUR, MINUTE, SECOND, GENERATE_SERIES, and more

### Aggregates (36 functions)
COUNT, SUM, AVG, MIN, MAX, STDDEV, VARIANCE, CORR, PERCENTILE_CONT, ARRAY_AGG, JSON_AGG, LISTAGG, GROUP_CONCAT, APPROX_COUNT_DISTINCT, and more

### Window (11 functions)
ROW_NUMBER, RANK, DENSE_RANK, NTILE, LAG, LEAD, FIRST_VALUE, LAST_VALUE, NTH_VALUE, PERCENT_RANK, CUME_DIST

### JSON (29 functions)
JSON_EXTRACT, JSONB_EXTRACT_PATH, JSON_BUILD_OBJECT, JSON_TABLE, JSONB_SET, JSONB_PRETTY, JSON_AGG, and more

### Array (17 functions)
ARRAY_LENGTH, ARRAY_POSITION, ARRAY_APPEND, ARRAY_CAT, UNNEST, STRING_TO_ARRAY, CARDINALITY, and more

### Geospatial (41 functions)
ST_DISTANCE, ST_CONTAINS, ST_INTERSECTS, ST_BUFFER, ST_UNION, ST_TRANSFORM, ST_ASGEOJSON, ST_MAKEPOINT, and more

### Text Search (12 functions)
TO_TSVECTOR, TO_TSQUERY, TS_RANK, TS_HEADLINE, SETWEIGHT, and more

## Optimizer Properties

Each function carries properties that guide optimization decisions:

| Property | Type | Description |
|----------|------|-------------|
| `deterministic` | bool | Same output for same input |
| `pure` | bool | No side effects, no external state dependency |
| `constant_foldable` | bool | Can evaluate at compile time if args are constants |
| `expensive` | bool | Computationally heavy (affects pushdown) |
| `strict` | bool | NULL input produces NULL output |
| `order_sensitive` | bool | Result depends on input order (aggregates) |
| `inlineable` | bool | Can be expanded at plan time |
| `cost_multiplier` | f64 | Cost relative to simple comparison (1.0) |

### Cost Multiplier Scale

| Range | Examples |
|-------|---------|
| 1.0 | ABS, LENGTH, SIGN, comparison |
| 1.5-2.0 | TRIM, SUBSTRING, EXTRACT, window functions |
| 3.0-5.0 | REPLACE, FORMAT, TO_CHAR, JSON operations |
| 5.0-10.0 | REGEXP_REPLACE, ST_DISTANCE, TS_RANK, cryptographic |
| 10.0-15.0 | TO_TSVECTOR, ST_UNION, JSON_TABLE, TS_HEADLINE |

## Optimization Rules

23 function-aware optimization rules in `rules/logical/function-optimization/`:

### Constant Folding (10 rules)
Evaluate pure functions at compile time when all arguments are constants:
- Arithmetic (ABS, SQRT, POWER, etc.)
- String (UPPER, LOWER, CONCAT, etc.)
- DateTime (EXTRACT, DATE_TRUNC, etc.)
- Comparison (=, <, >, BETWEEN, etc.)
- Boolean (AND, OR, NOT)
- COALESCE/NULLIF/CASE
- CAST/type conversions
- NULL propagation through strict functions
- Trigonometric functions
- JSON functions

### Expensive Function Pushdown (5 rules)
Control where expensive functions execute in the plan:
- Move expensive projections above filters
- Avoid pushing through row-multiplying joins
- Cache results of repeated deterministic calls
- Push LIMIT below expensive projections
- Lazy evaluation in OR/CASE (cheap branches first)

### Function-Index Matching (8 rules)
Connect function predicates to expression/specialized indexes:
- Expression index exact match (LOWER(col) = val)
- Expression index range match (EXTRACT(year) BETWEEN)
- GIN trigram for LIKE/ILIKE
- Collation-aware matching
- Computed column index matching
- JSON path expression indexes
- Spatial function to index predicate conversion
- Text search GIN index matching

## Usage

```rust
use ra_catalog::{FunctionCatalog, DatabaseSystem, FunctionCategory};

let catalog = FunctionCatalog::with_builtins();

// Look up a function
let abs = catalog.lookup("ABS").unwrap();
assert!(abs.properties.deterministic);

// Find expensive functions to avoid pushing below filters
let expensive = catalog.expensive_functions();

// Functions available in PostgreSQL
let pg_fns = catalog.by_database(DatabaseSystem::PostgreSQL);

// All aggregate functions
let aggs = catalog.by_category(FunctionCategory::Aggregate);
```

## Database Coverage

| Database | Functions |
|----------|-----------|
| PostgreSQL | 200+ (most complete) |
| MySQL | ~120 |
| SQLite | ~80 |
| SQL Server | ~100 |
| Oracle | ~90 |
