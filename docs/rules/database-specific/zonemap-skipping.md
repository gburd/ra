# Rule: MonetDB Zone Map Data Skipping

**Category:** database-specific/monetdb
**File:** `rules/database-specific/monetdb/zonemap-skipping.rra`

## Metadata

- **ID:** `monetdb-zonemap-skipping`
- **Version:** "1.0.0"
- **Databases:** monetdb
- **Tags:** database-specific, monetdb, zonemap, min-max, data-skipping, lightweight-index
- **Authors:** "Moerkotte 1998", "RA Contributors"


# MonetDB Zone Map Data Skipping

## Description

Uses lightweight min/max metadata per column zone (contiguous group of
values) to skip entire zones that cannot contain qualifying rows.
Unlike full indexes, zone maps require no maintenance on updates
(just extend the zone's min/max) and consume negligible space (two
values per zone). MonetDB maintains zone maps automatically as columns
are loaded and cracked.

**When to apply**: Range predicates on columns where the data has
some degree of clustering or ordering. Effective when zones can be
eliminated early, skipping large amounts of data.

**Why it works**: If a zone's max value is 50 and the predicate is
x > 100, the entire zone (potentially millions of rows) can be
skipped without examining any individual values. Zone maps trade
precision for zero maintenance cost -- they produce false positives
(zones that match but contain no qualifying rows) but never false
negatives.

**Database version**: MonetDB 11.x+ (zone maps integrated with imprints)

## Relational Algebra

```algebra
-- Without zone maps:
sigma[x > 100](scan(R))
  -> scan all N rows, compare each

-- With zone maps:
sigma[x > 100](zonemap_scan(R))
  -> for each zone z in R:
       if z.max > 100: scan zone, apply predicate
       else: skip zone entirely
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("monetdb-zonemap-skip";
    "(filter (compare ?op ?col ?val) (scan ?table))" =>
    "(zonemap-skip-scan ?op ?col ?val ?table)"
    if is_database("monetdb")
    if has_zonemap("?col")
    if predicate_is_range("?op")
),
```

## Preconditions

```rust
fn applicable(
    column: &Column,
    predicate: &Predicate,
) -> bool {
    // Column must have zone maps
    if !column.has_zonemap() {
        return false;
    }

    // Predicate must be range-based
    matches!(predicate, Predicate::Gt(..) | Predicate::Lt(..)
        | Predicate::Gte(..) | Predicate::Lte(..)
        | Predicate::Between(..) | Predicate::Eq(..))
}
```

**Restrictions:**
- Zone maps only help range/equality predicates (not LIKE, UDFs)
- Randomly ordered data defeats zone maps (all zones overlap)
- Zone size affects granularity: too large = low skip rate, too small = high metadata
- Boolean columns (2 values) rarely benefit from zone maps

## Cost Model

```rust
fn estimated_benefit(
    total_rows: f64,
    zone_size: f64,
    selectivity: f64,
    data_clustering: f64, // 0=random, 1=sorted
) -> f64 {
    let num_zones = total_rows / zone_size;
    let full_scan_cost = total_rows * 1.0;

    // Zones that can be skipped depends on clustering
    let skip_fraction = (1.0 - selectivity) * data_clustering;
    let scanned_zones = num_zones * (1.0 - skip_fraction);
    let scanned_rows = scanned_zones * zone_size;

    // Zone map check cost: negligible (2 comparisons per zone)
    let zonemap_cost = num_zones * 0.01;
    let scan_cost = scanned_rows * 1.0;
    let total_zonemap_cost = zonemap_cost + scan_cost;

    if full_scan_cost > total_zonemap_cost {
        (full_scan_cost - total_zonemap_cost) / full_scan_cost
    } else {
        0.0
    }
}
```

**Typical benefit**: 50-99% for selective range predicates on sorted or
clustered columns. 0% on randomly ordered data.

## Test Cases

```sql
-- Positive: time-series data (naturally ordered)
SELECT * FROM sensor_data
WHERE timestamp > '2024-06-01' AND timestamp < '2024-06-02';
-- Zone maps skip all zones outside date range
-- 99%+ of data skipped for single-day query on year of data

-- Positive: partially ordered data (after cracking)
SELECT * FROM transactions WHERE amount > 10000;
-- After cracking at various thresholds, zones have tighter min/max
-- Zone maps skip low-value zones efficiently
```

```sql
-- Negative: randomly distributed values
SELECT * FROM hash_table WHERE hash_value > 500000;
-- All zones span full value range, no skipping possible
```

## References

Moerkotte, "Small Materialized Aggregates: A Light Weight Index
Structure for Data Warehousing", VLDB 1998
Sidirourgos, Kersten, "Column Imprints: A Secondary Index Structure",
SIGMOD 2013
Sun et al., "Fine-Grained Statistics for Storage-Level Data Skipping",
SIGMOD 2014
