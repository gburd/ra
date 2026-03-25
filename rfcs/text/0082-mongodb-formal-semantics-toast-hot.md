# RFC 0082: MongoDB Formal Semantics with PostgreSQL TOAST/HOT Integration

- Start Date: 2026-03-25
- Author: Ra Research Team
- Status: Implemented
- Tracking Issue: TBD
- Implementation: `crates/ra-core/src/document_algebra.rs` (1,880 lines)

## Summary

Ra implements a formal document query algebra based on academic research into MongoDB query semantics. The algebra provides provably correct rewrite rules for document operations (selection, projection, unwind, grouping, lookup joins, sorting, limiting) with integrated PostgreSQL TOAST (The Oversized-Attribute Storage Technique) and HOT (Heap-Only Tuples) cost modeling. This enables Ra to optimize document-oriented queries (JSONB in PostgreSQL, DocumentDB) with formal correctness guarantees and awareness of storage-level performance characteristics.

## Motivation

Document databases like MongoDB and PostgreSQL's JSONB support have complex query semantics that differ from relational algebra. Optimizing these queries requires:

1. **Formal semantic foundation**: Rewrite rules must preserve query meaning under document data model semantics (nested documents, array unwinding, field paths).

2. **TOAST awareness**: PostgreSQL stores large JSONB values out-of-line using TOAST, incurring 2x I/O penalty. Avoiding TOASTed column access in hot paths is critical.

3. **HOT update optimization**: PostgreSQL's HOT updates avoid index maintenance when updated columns are not indexed. Document updates that modify non-indexed paths should prefer HOT-eligible strategies.

4. **Array semantics**: MongoDB's `$unwind` operator expands array fields into multiple documents with formal cardinality rules.

5. **Nested field navigation**: Field paths like `address.city.zipcode` navigate through nested structure with well-defined semantics.

6. **Aggregation pipeline optimization**: MongoDB aggregation pipelines compose operators with clear optimization opportunities (predicate pushdown, projection pruning, sort elimination).

**Research Foundation:**

This RFC implements formal semantics from:
- Botoeva et al., "A Formal Presentation of MongoDB" (SEBD 2016)
- Atzeni et al., "A Framework for Semi-structured Data" (2017)

## Guide-level explanation

### Document Data Model

A document is a partial function from field names (strings) to values. Values are:
- Atomic: `null`, `bool`, `int`, `float`, `string`, `timestamp`
- Nested: Another document
- Array: Ordered sequence of values

Field paths use dot notation to navigate nesting:
```
{
  "order_id": 123,
  "customer": {
    "name": "Alice",
    "address": {
      "city": "Boston"
    }
  },
  "items": [
    {"product": "Widget", "qty": 5},
    {"product": "Gadget", "qty": 2}
  ]
}
```

Field path `customer.address.city` evaluates to `"Boston"`.

### Document Query Operators

Following Botoeva et al., the algebra defines operators over multisets of documents:

**Selection** (`σ_φ`): Filter documents matching predicate φ
```
σ_{status = "shipped"}(orders)
```

**Projection** (`π_F`): Retain only fields in set F
```
π_{order_id, total}(orders)
```

**Unwind** (`μ_f`): Flatten array field f, producing one document per array element
```
μ_items(orders)  // Expands to one doc per item
```

**Grouping** (`γ_{G,A}`): Group by fields G, compute aggregates A
```
γ_{category, {total: SUM(price)}}(products)
```

**Lookup** (`λ_{C',k}`): Join with collection C' on key k (like LEFT OUTER JOIN)
```
λ_{customers, customer_id}(orders)
```

**AddFields** (`ρ_{f,e}`): Add computed field f with expression e
```
ρ_{total, price * qty}(items)
```

**Sort** (`τ_K`): Order by keys K
```
τ_{order_date DESC}(orders)
```

**Limit** (`δ_n`): Take first n documents
```
δ_10(results)
```

### Formal Rewrite Rules

Each operator has formal denotational semantics `[[op]](C)` where C is a multiset of documents. Rewrite rules preserve semantic equivalence:

**Rule 1: Selection pushdown through projection**
```
σ_φ(π_F(C)) ≡ π_F(σ_φ(C))  when φ references only fields in F
```

**Rule 2: Projection elimination**
```
π_F(π_G(C)) ≡ π_{F ∩ G}(C)
```

**Rule 3: Selection merging**
```
σ_φ(σ_ψ(C)) ≡ σ_{φ ∧ ψ}(C)
```

**Rule 4: Unwind-selection commutation**
```
σ_φ(μ_f(C)) ≡ μ_f(σ_φ'(C))  when φ references only non-array fields
```

**Rule 5: Limit-sort fusion**
```
δ_n(τ_K(C)) → top-n heap algorithm (avoid full sort)
```

### TOAST Integration

PostgreSQL TOAST (The Oversized-Attribute Storage Technique) stores large values out-of-line:

**TOAST Strategies:**
- **PLAIN**: Never compress or store externally (small values)
- **EXTENDED**: Compress first, then store externally if still large
- **EXTERNAL**: Store externally without compression
- **MAIN**: Compress but avoid external storage if possible

**I/O Cost Model:**
```
access_cost = base_cost + (is_toasted ? 2.0 * base_cost : 0)
```

Accessing TOASTed columns incurs 2x I/O: one fetch for the main tuple, one for the TOAST chunk pointer.

**Optimization Strategy:**
- Track which JSONB columns are likely TOASTed (avg size > 2KB)
- Avoid accessing TOASTed columns in tight loops
- Push projections down to eliminate unused TOASTed fields early
- Use GIN indexes on JSONB paths to avoid document access

**Example:**
```sql
-- Before: Accesses entire TOASTed JSONB column
SELECT data->>'status' FROM large_docs WHERE id = 123;

-- After: Uses GIN index to extract single field
SELECT status FROM large_docs_status_idx WHERE id = 123;
```

### HOT Updates

PostgreSQL's HOT (Heap-Only Tuple) updates avoid index maintenance when:
1. The updated columns are not indexed
2. The new tuple fits on the same page as the old tuple

**HOT Eligibility Check:**
```rust
fn is_hot_eligible(update_path: &FieldPath, indexed_paths: &[FieldPath]) -> bool {
    // HOT-eligible if update path is not a prefix of any indexed path
    !indexed_paths.iter().any(|idx_path| {
        update_path.is_prefix_of(idx_path) || idx_path.is_prefix_of(update_path)
    })
}
```

**Example:**
```sql
-- Table: events (data JSONB)
-- Index: GIN(data) WHERE data->>'type' = 'click'

-- HOT-eligible: Updates non-indexed field
UPDATE events SET data = jsonb_set(data, '{timestamp}', ...) WHERE id = 123;

-- NOT HOT-eligible: Updates indexed field 'type'
UPDATE events SET data = jsonb_set(data, '{type}', ...) WHERE id = 123;
```

**Cost Savings:**
```
hot_update_cost = tuple_write_cost
non_hot_update_cost = tuple_write_cost + index_maintenance_cost * num_indexes
```

### Pipeline Optimization

MongoDB aggregation pipelines compose operators:
```javascript
db.orders.aggregate([
  {$match: {status: "shipped"}},        // Selection
  {$unwind: "$items"},                  // Array expansion
  {$group: {_id: "$customer_id",        // Grouping
            total: {$sum: "$items.price"}}},
  {$sort: {total: -1}},                 // Sorting
  {$limit: 10}                          // Limit
])
```

The optimizer applies rewrite rules:
1. Push `$match` before `$unwind` (reduce expanded documents)
2. Eliminate unused fields after `$unwind` (projection pruning)
3. Fuse `$sort` + `$limit` into top-N heap
4. Push `$group` aggregation to index scan when possible

## Reference-level explanation

### Implementation: `document_algebra.rs`

**Core Types:**

```rust
pub struct FieldPath {
    segments: Vec<String>,  // ["address", "city"]
}

pub enum DocPredicate {
    Eq(FieldPath, DocValue),
    Lt(FieldPath, DocValue),
    In(FieldPath, Vec<DocValue>),
    And(Vec<DocPredicate>),
    // ... 12 total predicates
}

pub enum DocOperator {
    Match { predicate: DocPredicate },
    Project { fields: Vec<FieldPath> },
    Unwind { field: FieldPath, preserve_null: bool },
    Group { key: Option<Vec<FieldPath>>, accumulators: Vec<DocAccumulator> },
    Lookup { from: String, local_field: FieldPath, foreign_field: FieldPath, as_field: String },
    AddFields { fields: Vec<(String, DocExpr)> },
    Sort { keys: Vec<(FieldPath, SortOrder)> },
    Limit { count: u64 },
    Skip { count: u64 },
    // ... 3 more operators
}

pub struct DocPipeline {
    collection: String,
    stages: Vec<DocOperator>,
}

impl DocPipeline {
    pub fn optimize(&self) -> Self { /* Apply rewrite rules */ }
}
```

**TOAST/HOT Types:**

```rust
pub struct ToastInfo {
    pub column: String,
    pub strategy: ToastStrategy,
    pub avg_size_bytes: u64,
}

pub enum ToastStrategy {
    Plain,
    Extended,
    External,
    Main,
}

pub struct HotEligibility {
    pub column: String,
    pub is_indexed: bool,
    pub indexed_paths: Vec<FieldPath>,
}

impl HotEligibility {
    pub fn is_hot_eligible(&self, update_path: &FieldPath) -> bool {
        // Check if update touches indexed paths
    }
}
```

**Cost Estimation:**

```rust
pub fn estimate_stage_cost(
    stage: &DocOperator,
    input_count: f64,
    toast_info: Option<&ToastInfo>,
) -> Cost {
    match stage {
        DocOperator::Match { .. } => {
            // CPU-only predicate evaluation
            Cost::cpu_only(input_count * PREDICATE_EVAL_COST_PER_DOC)
        }
        DocOperator::Project { fields } => {
            // I/O cost for accessing fields
            let mut cost = Cost::default();
            for field in fields {
                if let Some(toast) = toast_info {
                    if field.root() == toast.column {
                        cost += estimate_toast_io_penalty(toast, input_count);
                    }
                }
            }
            cost
        }
        DocOperator::Unwind { .. } => {
            // Array expansion: increases output count
            Cost::cpu_only(input_count * UNWIND_COST_PER_DOC)
        }
        // ... all other operators
    }
}

pub fn estimate_pipeline_cost(
    pipeline: &DocPipeline,
    initial_count: f64,
    toast_info: Option<&ToastInfo>,
) -> Cost {
    let mut total_cost = Cost::default();
    let mut current_count = initial_count;

    for stage in &pipeline.stages {
        total_cost += estimate_stage_cost(stage, current_count, toast_info);
        current_count = estimate_output_count(stage, current_count);
    }

    total_cost
}
```

### Integration with Ra

**E-graph Integration:**

The document operators can be represented in Ra's e-graph:
```
(doc-match ?pred ?input)     → Selection
(doc-project ?fields ?input) → Projection
(doc-unwind ?field ?input)   → Unwind (specialized Unnest)
```

Rewrite rules translate document algebra patterns into relational equivalents when beneficial.

**Dialect Translation:**

PostgreSQL JSONB queries map to document operators:
```sql
SELECT jsonb_path_query(data, '$.items[*]') FROM docs WHERE data->>'status' = 'active'
```
Translates to:
```
DocPipeline::new("docs")
    .then(DocOperator::Match {
        predicate: DocPredicate::Eq(FieldPath::new("status"), DocValue::String("active"))
    })
    .then(DocOperator::Unwind {
        field: FieldPath::new("items"),
        preserve_null: false,
    })
```

### Testing

48 unit tests cover:
- Field path operations (depth, prefix checking, display)
- Predicate evaluation (eq, ne, lt, in, regex, and, or, elemMatch)
- Operator cost estimation (match, project, unwind, group, sort, limit)
- TOAST I/O penalty calculation
- HOT eligibility determination
- Output count estimation per operator
- Pipeline cost accumulation
- Pipeline optimization (predicate merging, projection elimination)

All tests pass with coverage of:
- Normal cases (typical pipelines)
- Edge cases (empty fields, null values, empty arrays)
- Error cases (invalid paths, type mismatches)

## Drawbacks

- **Complexity**: 1,880 lines adds maintenance burden
- **Limited applicability**: Benefits only document-oriented workloads
- **Formal semantics gap**: Full MongoDB semantics are more complex than the subset modeled here
- **TOAST estimation**: Avg size heuristics may be inaccurate without true statistics
- **HOT update detection**: Requires knowledge of update patterns, which may not be available at planning time

## Rationale and alternatives

### Why This Design?

**Formal foundation**: Academic research provides proven correct rewrite rules, reducing risk of semantic bugs.

**TOAST/HOT integration**: PostgreSQL-specific optimizations deliver measurable performance gains for document workloads.

**Modular**: `document_algebra.rs` is self-contained and can be used independently of the main optimizer.

### Alternative Approaches

**1. Pattern matching on SQL strings**: Match JSONB function calls in SQL AST
   - Rejected: Fragile, no formal guarantees, misses optimization opportunities

**2. Embed MongoDB query engine**: Use MongoDB's own optimizer
   - Rejected: Heavy dependency, no relational integration, licensing concerns

**3. Heuristic-only optimizations**: Apply common rewrites without formal basis
   - Rejected: Risk of semantic bugs, hard to verify correctness

### Impact of Not Doing This

Document workloads would continue to suffer from:
- Black-box JSONB function costs (wrong join ordering)
- Missed TOAST avoidance opportunities (2x unnecessary I/O)
- Missed HOT update eligibility (unnecessary index maintenance)
- No pipeline optimization (full materialization between stages)

## Prior art

### MongoDB Query Optimizer

MongoDB's aggregation pipeline optimizer performs:
- Predicate pushdown through $unwind
- Projection pruning
- $sort + $limit fusion into top-N
- Index selection for $match stages

### PostgreSQL JSONB

PostgreSQL provides:
- GIN indexes on JSONB paths
- `jsonb_path_query` for JSONPath evaluation
- Statistics collection on JSONB columns
- But no high-level pipeline optimization

### DocumentDB (AWS)

Amazon DocumentDB (MongoDB-compatible) runs on PostgreSQL with:
- BSON storage in BYTEA columns
- Custom GIN operator classes
- Query rewriting from MongoDB to SQL
- But limited optimization beyond basic translation

## Unresolved questions

- Should the optimizer attempt to materialize intermediate pipeline stages for re-use across multiple queries? (Deferred to caching RFC)
- How should the cost model handle compressed TOAST values (EXTENDED strategy)? (Currently assumes no compression benefit)
- Should HOT eligibility checks be dynamic (query-time) or static (schema-time)? (Currently static)

## Future possibilities

- **Materialized pipeline stages**: Cache intermediate results for repeated queries
- **Adaptive TOAST strategy**: Recommend TOAST strategy changes based on access patterns
- **Cross-collection join optimization**: Optimize $lookup across multiple collections
- **Pipeline parallelization**: Execute independent pipeline stages in parallel
- **DocumentDB dialect**: Direct translation of MongoDB queries to Ra plans

## Performance Impact

Expected speedups for document workloads:

| Optimization | Scenario | Expected Gain |
|--------------|----------|---------------|
| TOAST avoidance | Projection eliminates 80% of TOASTed columns | 1.5-2x |
| HOT update path | Updates to non-indexed JSONB paths | 2-5x (fewer indexes) |
| Predicate pushdown | Selection before $unwind | 5-10x (fewer expanded docs) |
| Top-N fusion | $sort + $limit | 2-100x (avoid full sort) |
| Projection pruning | Eliminate unused fields after $unwind | 1.5-3x |

## Implementation Status

✅ Implemented in `crates/ra-core/src/document_algebra.rs` (1,880 lines)
✅ 48 unit tests (all passing)
✅ Exported from `ra-core` public API
✅ Formal semantics based on Botoeva et al. and Atzeni et al.
✅ TOAST/HOT integration with cost modeling
✅ Pipeline optimization with rewrite rules

## References

1. Botoeva, Elena, et al. "A Formal Presentation of MongoDB (Extended Version)." SEBD 2016.
2. Atzeni, Paolo, et al. "A Framework for Semi-structured Data." 2017.
3. PostgreSQL TOAST documentation: https://www.postgresql.org/docs/current/storage-toast.html
4. PostgreSQL HOT updates: https://git.postgresql.org/gitweb/?p=postgresql.git;a=blob;f=src/backend/access/heap/README.HOT
5. MongoDB Aggregation Pipeline: https://www.mongodb.com/docs/manual/core/aggregation-pipeline/
6. DocumentDB (AWS): https://docs.aws.amazon.com/documentdb/
