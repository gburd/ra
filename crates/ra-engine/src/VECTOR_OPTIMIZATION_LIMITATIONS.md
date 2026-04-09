# Vector Optimization Known Limitations

## Issue #1: CAST Not Supported ❌

**Query:**
```sql
ORDER BY embedding <-> '[0.1, 0.2, ...]'::vector
```

**Error:** "CAST expressions are not yet supported in the e-graph representation"

**Why:** The `::vector` type cast creates an `Expr::Cast` which fails during e-graph conversion (egraph.rs line ~2130).

**Workaround:** Remove the cast - the optimizer doesn't need it:
```sql
-- Works:
ORDER BY embedding <-> '[0.1, 0.2, ...]'  -- No ::vector needed
```

---

## Issue #2: Project/Filter Between Sort and Scan 🔄

**Query:**
```sql
SELECT id, title FROM articles        -- ← Project
WHERE category = 'science'             -- ← Filter
ORDER BY embedding <-> '[...]'
LIMIT 10;
```

**Problem:** Vector rule expects `(limit (sort (scan)))` but gets `(limit (sort (project (filter (scan)))))`.

**Current Behavior:** Query optimizes other parts (e.g., BitmapIndexScan for filter) but **vector-topk-to-knn rule doesn't match**.

### Why This Happens

The rule pattern is:
```rust
"(limit ?k ?offset
   (sort ... (scan ?table)))"  // Direct scan only
```

But your query structure is:
```
Limit
└─ Sort
   └─ Project      ← Blocks pattern matching!
      └─ Filter
         └─ Scan
```

### Workarounds

**Option 1: Remove Projection** (Use `SELECT *`)
```sql
-- This will optimize:
SELECT * FROM articles
WHERE category = 'science'
ORDER BY embedding <-> '[0.1, 0.2, ...]'
LIMIT 10;
```

**Option 2: Remove Filter** (Pure KNN)
```sql
-- This will optimize:
SELECT id, title FROM articles
ORDER BY embedding <-> '[0.1, 0.2, ...]'
LIMIT 10;
```

**Option 3: Apply Manually**
The optimizer already found the best filter strategy (BitmapIndexScan). You can manually combine that insight with vector KNN in your application logic.

---

## What Works Today ✅

### ✅ Pure Vector KNN
```sql
SELECT * FROM items
ORDER BY embedding <-> '[1,2,3]'
LIMIT 10;

→ Optimizes to: vector_knn_scan
```

### ✅ Vector with Post-Filter
```sql
SELECT * FROM items
WHERE created_at > '2024-01-01'
ORDER BY embedding <-> '[1,2,3]'
LIMIT 10;

→ Rule: vector-postfilter (filter after KNN)
```

### ✅ Range Queries
```sql
SELECT * FROM items
WHERE embedding <-> '[1,2,3]' < 0.5;

→ Optimizes to: vector_range_scan
```

---

## Future Improvements

### Short Term: Add Project-Aware Rules

Add rule to match with Project:
```rust
rw!("vector-topk-with-project";
    "(limit ?k ?offset
       (sort (list (sort-key (vector-distance ?m ?c ?t) ?o ?n))
         (project ?cols (scan ?table))))" =>
    "(limit ?k ?offset
       (project ?cols (vector-knn ?table ?c ?t ?k)))"
);
```

**Challenge:** Need to handle all combinations:
- `sort → project → scan`
- `sort → project → filter → scan`
- `sort → filter → scan`
- `sort → scan`

This explodes the number of rules needed.

### Medium Term: Pattern Variables for Any Input

Use e-graph's ability to match arbitrary subtrees:
```rust
rw!("vector-topk-any-input";
    "(limit ?k ?offset
       (sort (list (sort-key (vector-distance ?m ?c ?t) ?o ?n))
         ?input))" =>
    "(limit ?k ?offset
       (vector-knn-over ?input ?c ?t ?k))"
);
```

**Challenge:** Need new `vector-knn-over` operator that works with arbitrary inputs, not just table names.

### Long Term: Smarter Pattern Matching

E-graph framework could support:
- Transitive matching (match through intermediate nodes)
- Conditional patterns (if X contains Y somewhere)
- Extract-and-transform (extract table from nested structure)

---

## Testing Matrix

| Query Structure | Optimizes? | Rule Applied |
|----------------|------------|--------------|
| `LIMIT → SORT → SCAN` | ✅ Yes | vector-topk-to-knn |
| `LIMIT → SORT → FILTER → SCAN` | ✅ Yes | vector-prefilter |
| `LIMIT → SORT → PROJECT → SCAN` | ❌ No | None (blocked by Project) |
| `LIMIT → SORT → PROJECT → FILTER → SCAN` | ❌ No | None (blocked by Project) |
| `FILTER → LIMIT → SORT → SCAN` | ✅ Yes | vector-postfilter |
| `ORDER BY ... ::vector` | ❌ No | CAST not supported |

---

## Recommendations

**For Users:**
1. Use `SELECT *` if possible when doing vector KNN
2. Remove `::vector` casts - they're not needed
3. Structure queries to match working patterns above

**For Developers:**
1. Add CAST support to e-graph (convert to identity function)
2. Add project-aware vector rules
3. Consider architectural change: vector-knn-over operator
4. Add integration tests for all query structures

---

## Related Files

- `crates/ra-engine/src/vector_rules.rs` - Rule definitions
- `crates/ra-engine/src/egraph.rs` - E-graph operators & conversion
- `crates/ra-engine/src/extract.rs` - Plan extraction

---

**Last Updated:** 2026-04-07
**Status:** Known limitation, workarounds available
