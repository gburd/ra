# RFC 0084: Oracle JSON Relational Duality View Optimization

- Start Date: 2026-03-25
- Author: Ra Research Team
- Status: Proposed
- Tracking Issue: TBD

## Summary

Ra should detect and optimize queries against Oracle JSON Relational Duality
views (introduced in Oracle Database 23ai) by understanding the bidirectional
mapping between JSON documents and relational tables. Duality views allow the
same data to be accessed as JSON documents or as relational rows. Ra can
improve query plans by choosing the optimal access path (document fetch vs
relational decomposition), pushing predicates into the JSON or relational
layer, optimizing update operations that maintain cross-view consistency, and
estimating costs for both access patterns.

## Motivation

Oracle JSON Relational Duality views are a significant advancement in hybrid
data modeling. They provide dual access to the same underlying normalized
relational tables--as JSON documents or as relational rows--with automatic
bidirectional synchronization. However, this duality creates optimization
challenges:

**1. Access path selection.** A query against a duality view can be executed
by either (a) fetching the pre-assembled JSON document and extracting fields,
or (b) decomposing the query into relational joins against the underlying
tables. The optimal choice depends on selectivity, the number of fields
accessed, join complexity, and available indexes.

**2. Predicate pushdown across boundaries.** Predicates on JSON document
fields map to predicates on relational columns, but the mapping is not
always 1:1. Nested JSON paths may require joins across multiple tables.
The optimizer must decide whether to push predicates into the JSON layer
(using JSON path expressions) or the relational layer (using column filters).

**3. Update consistency overhead.** Duality views maintain atomic consistency
across all underlying tables. An update to a single JSON document field may
require updating multiple table rows. The optimizer must estimate this fan-out
cost and choose the cheapest update strategy.

**4. Nested document assembly cost.** Assembling a JSON document from multiple
normalized tables requires joins. For queries that access only a subset of
document fields, partial assembly can avoid unnecessary joins.

**5. Multiple views over shared tables.** Different duality views can expose
different hierarchical projections of the same tables. The optimizer must
recognize when two queries on different views can share scan results.

**Expected impact:**

| Pattern | Current | Optimized | Gain |
|---------|---------|-----------|------|
| Point lookup by document ID | Full JSON assembly | Direct PK lookup | 2-5x |
| Filter on nested field | Full document scan | Predicate pushdown to base table | 5-20x |
| Partial field access (3/20 fields) | Full document assembly | Partial join elimination | 2-8x |
| Bulk update via JSON | Per-document decomposition | Batch relational update | 3-10x |
| Cross-view join | Two full assemblies | Shared base table scan | 2-4x |

## Design

### Duality View Model

A duality view defines a mapping from relational tables to JSON documents:

```
DualityView {
    name: "orders_dv"
    root_table: "orders"
    field_mappings: [
        JsonField("_id")       -> Column("orders", "order_id"),
        JsonField("customer")  -> NestedView {
            table: "customers",
            join: "orders.customer_id = customers.id",
            fields: [
                JsonField("name") -> Column("customers", "name"),
                JsonField("email") -> Column("customers", "email"),
            ]
        },
        JsonField("items")     -> NestedArray {
            table: "order_items",
            join: "orders.order_id = order_items.order_id",
            fields: [
                JsonField("product") -> Column("order_items", "product_name"),
                JsonField("qty")     -> Column("order_items", "quantity"),
            ]
        },
    ]
    updatable: INSERT | UPDATE | DELETE,
}
```

### Access Path Decision

The optimizer chooses between two fundamental access paths:

1. **Document path**: Fetch the assembled JSON document, apply JSON path
   predicates, extract requested fields. Best for: point lookups by document
   ID, queries accessing most document fields, CRUD operations.

2. **Relational path**: Decompose the query into joins against underlying
   tables, apply column predicates, assemble only requested fields. Best for:
   selective queries on indexed columns, partial field access, analytical
   queries with aggregation.

The cost model compares:
- Document path cost = document_fetch_cost + json_extraction_cost * n_fields
- Relational path cost = sum(table_scan_costs) + join_costs + assembly_cost

### Optimization Rules

1. **Duality view detection**: Recognize when a scan targets a duality view
   and annotate the e-graph with view metadata.

2. **Document-to-relational decomposition**: Rewrite a full document fetch
   into relational joins when predicate pushdown or partial field access
   makes the relational path cheaper.

3. **Relational-to-document collapse**: When all fields are accessed and no
   selective predicates exist, collapse relational joins back into a
   document fetch.

4. **Predicate pushdown for JSON fields**: Push predicates on JSON document
   fields down to the corresponding relational columns.

5. **Partial assembly elimination**: When only a subset of fields are
   requested, eliminate joins to tables whose columns are not referenced.

6. **Update fan-out estimation**: For DML operations, estimate the number
   of base table rows affected and choose batch vs per-document strategy.

### Cost Model Parameters

```
document_fetch_base_cost = 5.0    // Single document retrieval
json_field_extract_cost  = 0.2    // Per-field extraction from JSON
json_predicate_cost      = 0.8    // Evaluating a JSON path predicate
relational_join_cost     = 3.0    // Per-join cost (base table join)
partial_assembly_cost    = 1.5    // Assembling partial document
update_fanout_cost       = 2.0    // Per-table update in consistency maintenance
```

## Implementation

New module: `crates/ra-engine/src/oracle_json_duality.rs` (~500 lines)

- `DualityView` struct modeling the JSON-to-relational mapping
- `DualityFieldMapping` enum for scalar, nested, and array mappings
- `AccessPath` enum (Document vs Relational)
- `DualityCostParams` for tuning the cost model
- `choose_access_path()` function comparing document vs relational cost
- `duality_rewrite_rules()` returning egg rewrite rules
- `estimate_document_cost()` and `estimate_relational_cost()` functions
- `estimate_update_cost()` for DML operations

Integration: Add `oracle_json_duality_rewrite_rules()` to
`crates/ra-engine/src/rewrite.rs::all_rules_unsorted()`.

## Testing

- 15+ unit tests covering:
  - Access path selection (document vs relational)
  - Predicate pushdown decisions
  - Partial field access optimization
  - Update cost estimation
  - Cost model parameter sensitivity
  - E-graph rewrite rule application
  - Edge cases (empty views, single-table views, deep nesting)

## References

- Oracle JSON Relational Duality Views documentation (Oracle 23ai)
- Oracle REST Data Services (ORDS) for document API access
- MongoDB API for Oracle Database compatibility layer
