# Rule: MonetDB Strimps String Filtering

**Category:** database-specific/monetdb
**File:** `rules/database-specific/monetdb/strimps-string-filter.rra`

## Metadata

- **ID:** `monetdb-strimps-string-filter`
- **Version:** "1.0.0"
- **Databases:** monetdb
- **Tags:** database-specific, monetdb, strimps, string, LIKE, filter, index
- **Authors:** "RA Contributors"


# MonetDB Strimps String Filtering

## Description

Strimps (STRing IMPrintS) are a lightweight index structure for
accelerating LIKE queries on string columns.  A strimp encodes a
bitset of character pair (bigram) presence per block of strings.
During a LIKE query, the optimizer checks each block's strimp against
the required bigrams and skips blocks that cannot contain matches.

**When to apply**: A LIKE predicate with a pattern that contains
known bigrams, and a strimps index has been built on the column.

**Why it works**: Checking a bitset is O(1) per block.  Blocks
without the required bigrams are skipped entirely, avoiding the
per-string regex evaluation.

**Database version**: MonetDB 11.41+

## Relational Algebra

```algebra
-- Before: full column LIKE scan
sigma[name LIKE '%Smith%'](scan(users.name))

-- After: strimps-filtered scan
sigma[name LIKE '%Smith%'](
    strimps_filter(users.name, bigrams=['Sm', 'mi', 'it', 'th']))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("monetdb-strimps-filter";
    "(filter (like ?col ?pattern) (scan ?table))" =>
    "(filter (like ?col ?pattern)
        (strimps-scan ?table ?col ?pattern))"
    if is_database("monetdb")
    if has_strimps_index("?col")
    if pattern_has_bigrams("?pattern")
),
```

## Preconditions

```rust
fn applicable(
    column: &Column,
    pattern: &str,
) -> bool {
    column.has_strimps_index()
    && extract_bigrams(pattern).len() >= 1
}
```

**Restrictions:**
- Single-character patterns have no bigrams; strimps cannot help
- Very common bigrams (e.g., 'th', 'he') provide low selectivity
- Strimps index must be pre-built with `CREATE IMPRINTS INDEX`

## Cost Model

```rust
fn estimated_benefit(
    total_blocks: f64,
    pruned_fraction: f64,
    per_string_regex_cost: f64,
    strings_per_block: f64,
) -> f64 {
    let full_cost = total_blocks * strings_per_block
        * per_string_regex_cost;
    let strimps_cost = total_blocks * (1.0 - pruned_fraction)
        * strings_per_block * per_string_regex_cost;
    full_cost - strimps_cost
}
```

**Typical benefit**: 2-10x for selective LIKE queries on large
string columns.

## Test Cases

```sql
-- Positive: LIKE with distinctive bigrams
SELECT * FROM users WHERE name LIKE '%Zyx%';
-- Strimps: bigrams 'Zy','yx' are rare; most blocks pruned
```

```sql
-- Negative: single-character pattern
SELECT * FROM users WHERE name LIKE '%a%';
-- No bigrams; strimps cannot help
```

## References

Moerkotte, G. et al. "String IMPrintS (Strimps)" (MonetDB blog)
Source: gdk/gdk_strimps.c
