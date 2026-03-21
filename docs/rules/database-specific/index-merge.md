# Rule: TiDB Index Merge

**Category:** database-specific/tidb
**File:** `rules/database-specific/tidb/index-merge.rra`

## Metadata

- **ID:** `tidb-index-merge`
- **Version:** "1.0.0"
- **Databases:** tidb
- **Tags:** index, merge, optimization, or
- **Authors:** "PingCAP TiDB Team", "RA Contributors"


# TiDB Index Merge

## Description

Merges multiple index lookups for OR conditions, scanning each index
separately and merging results, avoiding full table scans when multiple
indexes are selective.

## Relational Algebra

```algebra
Filter[pred1 OR pred2](Scan[table])
  -> Union(
       IndexScan[idx1](Filter[pred1](table)),
       IndexScan[idx2](Filter[pred2](table))
     )
  where has_index(pred1) AND has_index(pred2)
```

## Implementation

```rust
fn use_index_merge(filter: &Filter, scan: &Scan) -> Option<IndexMerge> {
    if let Or(preds) = &filter.predicate {
        let indexes: Vec<_> = preds.iter()
            .filter_map(|p| scan.table.matching_index(p))
            .collect();
        if indexes.len() == preds.len() {
            Some(IndexMerge::new(indexes))
        } else {
            None
        }
    } else {
        None
    }
}
```

## Cost Model

Combines selective index scans instead of full table scan.

## Test Cases

```sql
-- Multiple indexed predicates with OR
SELECT * FROM products
WHERE category_id = 5 OR brand_id = 100;
-- Index merge: Scan category index + brand index, union results
```

## References
- Source: `pkg/planner/core/exhaust_physical_plans.go` (getIndexMergeTask)
- TiDB Docs: https://docs.pingcap.com/tidb/stable/index-merge
