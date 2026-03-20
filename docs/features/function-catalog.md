# Function Catalog

This document describes the function catalog in the `ra-catalog` crate,
which provides the optimizer with metadata about SQL functions: their
signatures, behavioral properties, and per-row cost estimates.

Source: `crates/ra-catalog/src/functions.rs`, `crates/ra-catalog/data/functions.toml`

## Purpose

The optimizer needs to know more than just a function's name and return
type. To make correct and efficient plans, it must answer:

- Can this function be evaluated at plan time? (constant folding)
- Is it safe to push this predicate below a join? (determinism, purity)
- How expensive is this function per row? (cost estimation)
- Can this function call match an expression index? (index matching)
- Does calling this function twice with the same input give the same
  result? (common subexpression elimination)

The function catalog answers all of these questions.

## Catalog Structure

### FunctionDefinition

Every function has a canonical upper-case name, a category, one or more
overloaded signatures, and a set of behavioral properties.

```rust
pub struct FunctionDefinition {
    pub name: String,                    // "ABS", "ST_DISTANCE"
    pub category: FunctionCategory,      // Scalar, Aggregate, Window, TableValued
    pub signatures: Vec<FunctionSignature>,
    pub properties: FunctionProperties,
}
```

### FunctionCategory

| Category      | Description                          | Examples                          |
|---------------|--------------------------------------|-----------------------------------|
| `Scalar`      | Row-level function                   | ABS, UPPER, CAST, ST_Distance    |
| `Aggregate`   | Consumes a group of rows             | COUNT, SUM, AVG, STRING_AGG      |
| `Window`      | Evaluated over a window frame        | ROW_NUMBER, RANK, LAG, LEAD      |
| `TableValued` | Returns a set of rows                | UNNEST, GENERATE_SERIES, JSONB_EACH |

### FunctionSignature

Each overload specifies argument types, return type, and whether the
last argument repeats (variadic).

```rust
pub struct FunctionSignature {
    pub args: Vec<DataType>,
    pub return_type: DataType,
    pub variadic: bool,          // e.g. COALESCE(a, b, c, ...)
}
```

Supported `DataType` variants: `Integer`, `Float`, `Decimal`, `Text`,
`Boolean`, `Date`, `Timestamp`, `Interval`, `Blob`, `Json`,
`Array(inner)`, `Geometry`, `TsVector`, `TsQuery`, `Any`.

### FunctionProperties

These properties drive optimizer decisions:

| Property            | Type  | Optimizer Use                                      |
|---------------------|-------|----------------------------------------------------|
| `deterministic`     | bool  | Same inputs always produce same output. Enables CSE, expression index matching, and memoization. |
| `inlineable`        | bool  | Function body can be expanded inline into the calling expression. |
| `expensive`         | bool  | High per-row cost (>5x baseline). Affects join ordering, predicate ordering, and pushdown decisions. |
| `pure`              | bool  | No side effects and no dependency on external state. Enables cross-block CSE and CTE factoring. |
| `constant_foldable` | bool  | Can be evaluated at plan time when all arguments are constants. |
| `cost_multiplier`   | f64   | Relative cost vs. a simple comparison (1.0). Used by the cost model for per-row estimates. |

**Property combinations and optimizer behavior:**

- `deterministic + constant_foldable`: enables constant folding.
  `ABS(-5)` becomes `5` at plan time.
- `deterministic + expensive`: enables memoization when NDV is low.
  `geocode(city)` on 50 distinct cities is cached.
- `pure + deterministic`: enables common subexpression elimination
  across query blocks.
- `!deterministic`: prevents constant folding and CSE.
  `RANDOM()`, `NOW()`, `NEXTVAL()` must be evaluated at runtime.
- `expensive`: predicates containing expensive functions are reordered
  so cheaper predicates run first (short-circuit AND).

## TOML Format

The built-in catalog is stored in `crates/ra-catalog/data/functions.toml`.
Each function is a `[[function]]` entry:

```toml
[[function]]
name = "ABS"
category = "Scalar"
deterministic = true
inlineable = true
expensive = false
pure = true
constant_foldable = true
cost_multiplier = 1.0

[[function.signature]]
args = ["Integer"]
return_type = "Integer"

[[function.signature]]
args = ["Float"]
return_type = "Float"
```

**Defaults** (fields can be omitted):
- `deterministic = true`
- `pure = true`
- `cost_multiplier = 1.0`
- `inlineable = false`
- `expensive = false`
- `constant_foldable = false`

## Loading the Catalog

```rust
use ra_catalog::functions::load_catalog_from_toml;

let toml_text = include_str!("../data/functions.toml");
let catalog = load_catalog_from_toml(toml_text)
    .expect("built-in catalog should parse");

// Lookup by name (case-insensitive)
let abs = catalog.lookup("abs").unwrap();
assert!(abs.properties.deterministic);

// Filter by category
let aggregates = catalog.by_category(FunctionCategory::Aggregate);

// Find all expensive functions
let expensive = catalog.expensive_functions();
```

## Extending the Catalog

To add a new function, append a `[[function]]` entry to `functions.toml`:

```toml
[[function]]
name = "MY_CUSTOM_FN"
category = "Scalar"
deterministic = true
expensive = true
pure = true
constant_foldable = true
cost_multiplier = 8.0

[[function.signature]]
args = ["Text", "Integer"]
return_type = "Text"
```

Or register programmatically:

```rust
catalog.register(FunctionDefinition {
    name: "MY_CUSTOM_FN".into(),
    category: FunctionCategory::Scalar,
    signatures: vec![FunctionSignature {
        args: vec![DataType::Text, DataType::Integer],
        return_type: DataType::Text,
        variadic: false,
    }],
    properties: FunctionProperties::expensive_pure(),
});
```

## Function-Aware Optimizations

The catalog enables these optimization rules (see
`rules/logical/function-optimization/`):

### Constant Folding

Evaluates pure, constant-foldable functions at plan time when all
arguments are literals. Covers math, string, datetime, comparison,
conditional, and nested function calls.

### Expensive Function Pushdown/Pullup

Moves expensive function evaluations above cardinality-reducing joins
(fewer rows = fewer evaluations). Pushes cheap function predicates below
joins to filter early.

### Predicate Ordering

Reorders AND predicates by `cost / (1 - selectivity)` so cheap selective
predicates short-circuit before expensive ones.

### Expression Index Matching

Matches `WHERE f(col) = val` predicates to expression indexes defined
on `f(col)`.

### Common Subexpression Elimination

Deduplicates identical deterministic function calls within a query block.
Extends to pure functions across query blocks.

### Result Caching

Wraps expensive deterministic functions in a memoization operator when
the input column has low NDV.

### Specialized Rules

- JSON: collapse nested extractions, match GIN indexes
- Array: match GIN containment, eliminate UNNEST/ARRAY_AGG round-trips
- Geospatial: match GiST indexes, add bounding-box pre-filters,
  rewrite `ST_Distance < r` to index-compatible `ST_DWithin`

## Coverage

The built-in catalog contains 270+ functions across these categories:

| Category       | Count | Examples                                    |
|----------------|-------|---------------------------------------------|
| Math           | 30    | ABS, SQRT, SIN, LOG, POWER, RANDOM         |
| String         | 30    | UPPER, SUBSTRING, REPLACE, REGEXP_MATCH     |
| Date/Time      | 25    | NOW, DATE_TRUNC, EXTRACT, TO_TIMESTAMP      |
| Type Conv.     | 10    | CAST, COALESCE, NULLIF, TO_CHAR             |
| Conditional    | 5     | CASE, IF, GREATEST, LEAST                   |
| Aggregate      | 30    | COUNT, SUM, AVG, STDDEV, PERCENTILE_CONT    |
| Window         | 12    | ROW_NUMBER, RANK, LAG, LEAD, NTH_VALUE      |
| JSON           | 25    | JSON_EXTRACT, JSONB_SET, JSONB_AGG          |
| Array          | 18    | ARRAY_AGG, UNNEST, ARRAY_CONTAINS           |
| Geospatial     | 30    | ST_Distance, ST_Contains, ST_Buffer         |
| Full-Text      | 10    | TO_TSVECTOR, TS_RANK, TS_HEADLINE           |
| System         | 15    | NEXTVAL, GEN_RANDOM_UUID, PG_TABLE_SIZE     |

## Integration with Cost Models

The `cost_multiplier` field feeds into the plan cost model. For a
projection that applies function `f` to `N` rows:

```
projection_cpu_cost = N * f.cost_multiplier * BASE_EXPR_COST
```

The cost model uses this to compare plans that differ in where functions
are evaluated (before vs. after joins, before vs. after aggregations).

See also: [cost-models.md](cost-models.md), [index-types.md](index-types.md).
