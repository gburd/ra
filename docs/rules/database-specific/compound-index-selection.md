# Rule: MongoDB Compound Index Selection

**Category:** database-specific/mongodb
**File:** `rules/database-specific/mongodb/compound-index-selection.rra`

## Metadata

- **ID:** `mongodb-compound-index-selection`
- **Version:** "1.0.0"
- **Databases:** mongodb
- **Tags:** index, compound, selection, esr
- **Authors:** "MongoDB Inc."


# MongoDB Compound Index Selection

## Description

Selects optimal compound index for queries using the ESR (Equality, Sort, Range)
rule: equality predicates should come first in index, followed by sort fields,
then range predicates. This maximizes index efficiency and minimizes documents
examined.

**When to apply**: Queries with multiple predicates combining equality, range,
and sort operations. Compound index design critically impacts performance.

**Why it works**: ESR ordering ensures: (1) equality predicates narrow to
specific index entries, (2) results are pre-sorted, (3) range scans occur
on pre-filtered data. Wrong order (e.g., range before equality) forces
scanning larger index portions.

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("mongodb-select-esr-index";
    "(sort ?sort-fields
       (filter (and (eq ?eq-field ?val) (range ?range-field ?bounds))
         (scan ?collection)))" =>
    "(sort ?sort-fields
       (index-scan
         (find-esr-index ?eq-field ?sort-fields ?range-field ?collection)
         (filter (and (eq ?eq-field ?val) (range ?range-field ?bounds)))))"
    if has-esr-compatible-index("?eq-field", "?sort-fields", "?range-field")
),
```

## Test Cases

### Positive: ESR-ordered index

```javascript
// Query: equality on status, sort by date, range on priority
// Optimal index: {status: 1, date: 1, priority: 1}
db.tasks.find({
  status: "active",
  priority: {$gte: 5}
}).sort({date: -1})

// Compound index follows ESR: Equality(status), Sort(date), Range(priority)
```

### Negative: Wrong index order

```javascript
// Bad index: {priority: 1, status: 1, date: 1} (Range first!)
// Must scan all priority >= 5, then filter status
// vs. ESR index: narrow to status="active" first
```

## References

**Documentation:**
- MongoDB Manual: "Create Indexes to Support Your Queries"
- ESR Rule: https://www.mongodb.com/docs/manual/tutorial/equality-sort-range-rule/
