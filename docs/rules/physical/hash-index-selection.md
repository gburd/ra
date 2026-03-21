# Rule: "Hash Index Selection for Equality Lookups"

**Category:** physical/index-selection
**File:** `rules/physical/index-selection/hash-index-selection.rra`

## Metadata

- **ID:** `hash-index-selection`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, cockroachdb
- **Tags:** index, hash, equality, point-lookup
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(filter (= ?col ?val) (scan ?table))"
    description: "Equality filter with hash index candidate"
  - type: "predicate"
    condition: "has_hash_index(?table, ?col)"
    description: "Hash index must exist on the column"
```


# Hash Index Selection for Equality Lookups

## Metadata
- **Rule ID**: `hash-index-selection`
- **Category**: Physical / Index Selection
- **Complexity**: O(1) average per lookup
- **Prerequisites**: Hash index on column; equality predicate only
- **Alternatives**: B-tree index, sequential scan

## Description

Hash indexes provide O(1) average-case lookup for equality predicates.
Unlike B-tree indexes which support range queries, ordering, and
prefix matching, hash indexes are specialized for exact-match lookups
(=, IN).

The hash index maps key values directly to heap tuple IDs through a
hash function. This makes point lookups faster than B-tree (no tree
traversal) but prevents range scans, ordering, and partial key matching.

PostgreSQL supports hash indexes (WAL-logged since PG 10). MySQL
InnoDB uses adaptive hash indexes internally. Oracle uses hash
clusters. CockroachDB supports hash-sharded indexes for avoiding
hotspots.

**When to apply:**
- Equality predicate (=) on indexed column
- High-cardinality column (unique or near-unique values)
- No range or ordering requirements

**When to avoid:**
- Range predicates (<, >, BETWEEN)
- ORDER BY on the indexed column
- Low cardinality (bitmap index better)
- Multi-column prefix matching

## Relational Algebra

```
filter[col = value](scan[T])
  -> hash-index-lookup[HI(col)](value)
     -> heap-fetch[T](tid)
```

## Implementation (egg rewrite rules)

```lisp
;; Use hash index for equality lookup
(rewrite (filter (= ?col ?val) (scan ?table))
  (heap-fetch ?table
    (hash-index-lookup (hash-index ?table ?col) ?val))
  :if (has-hash-index ?table ?col)
  :if (is-equality-pred (= ?col ?val)))

;; Prefer hash over B-tree for equality on high-cardinality
(rewrite (btree-index-lookup ?idx ?val)
  (hash-index-lookup (hash-index-for ?idx) ?val)
  :if (has-hash-index-equivalent ?idx)
  :if (is-equality-only ?val)
  :if (> (cardinality ?idx) 10000))

;; Hash index for IN-list
(rewrite (filter (in ?col ?values) (scan ?table))
  (union-all
    (map ?values
      (lambda (?v)
        (heap-fetch ?table
          (hash-index-lookup (hash-index ?table ?col) ?v)))))
  :if (has-hash-index ?table ?col)
  :if (< (count ?values) 100))
```

## Cost Model

```rust
pub fn cost_hash_index_lookup(
    num_lookups: u64,
    avg_matches_per_lookup: f64,
    heap_page_cost: f64,
    hardware: &HardwareModel,
) -> Cost {
    let hash_cost = Cost::cpu(num_lookups * 5);
    let bucket_access = Cost::io(num_lookups as f64 * hardware.random_page_cost());
    let heap_fetch = Cost::io(
        num_lookups as f64 * avg_matches_per_lookup * heap_page_cost
    );
    hash_cost + bucket_access + heap_fetch
}

pub fn cost_btree_lookup(
    num_lookups: u64,
    tree_height: u64,
    avg_matches_per_lookup: f64,
    heap_page_cost: f64,
    hardware: &HardwareModel,
) -> Cost {
    let tree_descent = Cost::io(
        num_lookups as f64 * tree_height as f64 * hardware.random_page_cost()
    );
    let heap_fetch = Cost::io(
        num_lookups as f64 * avg_matches_per_lookup * heap_page_cost
    );
    tree_descent + heap_fetch
}
```

**Typical benefit**: 50-95% faster than sequential scan; ~30% faster than B-tree for equality

## Test Cases

### Positive: Single equality lookup
```sql
CREATE INDEX USING HASH ON users(email);

SELECT * FROM users WHERE email = 'user@example.com';
-- O(1) hash lookup vs O(log n) B-tree traversal
-- Single bucket access + heap fetch
```

### Positive: IN-list with hash index
```sql
SELECT * FROM products WHERE sku IN ('A001', 'B002', 'C003');
-- 3 hash lookups; each O(1)
```

### Negative: Range query
```sql
SELECT * FROM orders WHERE date > '2024-01-01';
-- Hash index cannot support range; B-tree required
```

### Negative: ORDER BY
```sql
SELECT * FROM users ORDER BY email LIMIT 10;
-- Hash index has no ordering; B-tree provides sorted access
```

## References

- PostgreSQL: Hash Indexes documentation (PG 10+ WAL support)
- MySQL: InnoDB Adaptive Hash Index
- Knuth, "The Art of Computer Programming, Vol. 3: Sorting and Searching"
