# Rule: "Volcano Enforcer Placement"

**Category:** physical/optimizer-framework
**File:** `rules/physical/optimizer-framework/volcano-enforcer-placement.rra`

## Metadata

- **ID:** `volcano-enforcer-placement`
- **Version:** "1.0.0"
- **Databases:** cockroachdb, mssql, greenplum
- **Tags:** volcano, cascades, enforcer, sort, exchange, physical-properties, classic
- **Authors:** "Goetz Graefe, William McKenna"


# Volcano Enforcer Placement

## Description

In the Volcano/Cascades framework, an "enforcer" is a physical operator that
establishes a required physical property that the child plan does not naturally
provide. The two canonical enforcers are Sort (establishes ordering) and
Exchange (establishes distribution/partitioning). The optimizer decides
whether to insert an enforcer or to use an implementation that naturally
provides the required property.

The enforcer placement decision is integrated into the top-down search: when
a parent operator requires a property (e.g., sorted input), the optimizer
considers both (a) requesting the property from the child and (b) requesting
any property from the child plus an enforcer. The cheaper option wins.

**When to apply**: When a physical operator requires a specific property from
its input (sort order for merge join, partitioning for distributed exchange)
that the input may or may not provide.

**Why it works**: Sometimes producing sorted output during a scan (index scan)
is cheaper than scanning unsorted and adding a sort. Other times, the reverse
is true. By explicitly modeling enforcers as cost alternatives, the optimizer
makes this tradeoff systematically rather than heuristically.

## Relational Algebra

```algebra
When optimizing group G with required property P:

Option A: Find plan in G that provides P naturally
  cost_A = FindBestPlan(G, P)

Option B: Find cheapest plan in G (any properties) + enforcer for P
  cost_B = FindBestPlan(G, ANY) + cost(Enforcer_P)

Choose min(cost_A, cost_B)

Enforcer types:
  Sort(keys):    establishes ordering on keys
  Exchange(dist): establishes distribution (hash, broadcast, round-robin)
  Materialize:   establishes reusability (for nested-loop inner)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

// Enforcer rules: when a property is required but not provided,
// insert an enforcer operator

rw!("enforcer-sort";
    "(require-sorted ?keys ?input)" =>
    "(sort ?keys ?input)"
    if !provides_sort("?input", "?keys")
),

rw!("enforcer-sort-elim";
    "(require-sorted ?keys ?input)" =>
    "?input"
    if provides_sort("?input", "?keys")
),

rw!("enforcer-exchange-hash";
    "(require-partitioned ?keys ?input)" =>
    "(exchange hash ?keys ?input)"
    if !provides_partitioning("?input", "?keys")
),

rw!("enforcer-exchange-broadcast";
    "(require-replicated ?input)" =>
    "(exchange broadcast ?input)"
    if !is_replicated("?input")
),

rw!("enforcer-exchange-elim";
    "(require-partitioned ?keys ?input)" =>
    "?input"
    if provides_partitioning("?input", "?keys")
),

// Full enforcer placement in Volcano context

struct EnforcerPlacement;

impl EnforcerPlacement {
    fn optimize_with_enforcers(
        &self,
        group: GroupId,
        required: &PhysicalProperties,
        memo: &mut MemoTable,
        cost_limit: f64,
    ) -> Option<(PhysicalPlan, f64)> {
        let mut best: Option<(PhysicalPlan, f64)> = None;

        // Option A: Find plan that naturally provides required props
        if let Some((plan, cost)) =
            memo.find_best_plan(group, required, cost_limit)
        {
            best = Some((plan, cost));
        }

        // Option B: Cheapest plan + enforcer
        let enforcer_cost = self.enforcer_cost(
            required, memo.group_stats(group),
        );

        let child_budget = best.as_ref()
            .map(|(_, c)| *c - enforcer_cost)
            .unwrap_or(cost_limit - enforcer_cost);

        if child_budget > 0.0 {
            if let Some((child_plan, child_cost)) =
                memo.find_best_plan(
                    group,
                    &PhysicalProperties::any(),
                    child_budget,
                )
            {
                let total = child_cost + enforcer_cost;
                let should_replace = best.as_ref()
                    .map(|(_, c)| total < *c)
                    .unwrap_or(true);

                if should_replace {
                    let enforced_plan =
                        self.add_enforcer(child_plan, required);
                    best = Some((enforced_plan, total));
                }
            }
        }

        best
    }

    fn enforcer_cost(
        &self,
        property: &PhysicalProperties,
        stats: &GroupStats,
    ) -> f64 {
        match property {
            PhysicalProperties::Sorted(keys) => {
                let n = stats.estimated_rows as f64;
                // External sort cost
                n * n.log2() * 0.000002 // 2us per comparison
            }

            PhysicalProperties::Partitioned(keys) => {
                let n = stats.estimated_rows as f64;
                let row_size = stats.avg_row_size as f64;
                // Network transfer cost
                n * row_size * 0.000001 // 1us per byte over network
            }

            PhysicalProperties::Replicated => {
                let n = stats.estimated_rows as f64;
                let row_size = stats.avg_row_size as f64;
                let nodes = stats.cluster_size as f64;
                // Broadcast: send to all nodes
                n * row_size * nodes * 0.000001
            }

            _ => 0.0,
        }
    }

    fn add_enforcer(
        &self,
        child: PhysicalPlan,
        property: &PhysicalProperties,
    ) -> PhysicalPlan {
        match property {
            PhysicalProperties::Sorted(keys) => {
                PhysicalPlan::Sort {
                    keys: keys.clone(),
                    input: Box::new(child),
                }
            }
            PhysicalProperties::Partitioned(keys) => {
                PhysicalPlan::Exchange {
                    kind: ExchangeKind::Hash,
                    keys: keys.clone(),
                    input: Box::new(child),
                }
            }
            PhysicalProperties::Replicated => {
                PhysicalPlan::Exchange {
                    kind: ExchangeKind::Broadcast,
                    keys: vec![],
                    input: Box::new(child),
                }
            }
            _ => child,
        }
    }
}
```

## Preconditions

```rust
fn applicable(
    stats: &Statistics,
    hw: &HardwareProfile,
) -> bool {
    // Enforcers are needed when physical properties are required
    stats.has_ordering_requirements
        || stats.has_partitioning_requirements
        || stats.has_merge_join_opportunity
}
```

**Restrictions:**
- Enforcer cost must be accurately modeled
- Sort enforcer: O(n log n) CPU + potential disk spill
- Exchange enforcer: network transfer cost proportional to data size
- Enforcer can be avoided if the child naturally provides the property

## Cost Model

```rust
fn estimated_benefit(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> f64 {
    let rows = stats.estimated_output_rows as f64;

    // Sort enforcer cost
    let sort_cost = rows * rows.log2().max(1.0) * 0.000002;

    // Alternative: index scan that provides the order
    let index_scan_cost = rows * 0.000005; // Random I/O per row

    // Benefit of avoiding enforcer
    if sort_cost > index_scan_cost {
        // Enforcer is expensive, natural order is cheap
        (sort_cost - index_scan_cost) / sort_cost
    } else {
        // Enforcer is cheap, natural order expensive
        // Enforcer is the better choice
        (index_scan_cost - sort_cost) / index_scan_cost
    }
}
```

**Typical benefit**: 30% to 5x by choosing between enforcer and natural property.

## Test Cases

### Positive: Sort enforcer eliminated by index scan

```sql
-- Clustered index on orders(order_date)
SELECT * FROM orders ORDER BY order_date;

-- Option A: SeqScan + Sort enforcer
--   Cost: 100,000 pages + sort(10M rows) = 100,000 + 230,000 = 330,000
-- Option B: IndexScan (naturally sorted)
--   Cost: 100,000 pages (sequential through index) = 100,000
-- Enforcer eliminated! Index provides the order.
```

### Positive: Sort enforcer cheaper than index scan

```sql
-- Non-clustered index on employees(last_name)
-- Need all columns (not index-only)
SELECT * FROM employees ORDER BY last_name;

-- Option A: SeqScan + Sort enforcer
--   Cost: 5,000 pages + sort(500K rows) = 5,000 + 45,000 = 50,000
-- Option B: Non-clustered IndexScan (random I/O)
--   Cost: 500,000 random page reads = 500,000
-- Enforcer wins! Sort is cheaper than random I/O.
```

### Positive: Exchange enforcer for distributed join

```sql
-- Distributed query, orders partitioned by order_id
-- Need to join on customer_id (different partition key)
SELECT * FROM orders o JOIN customers c ON o.customer_id = c.id;

-- Option A: Repartition orders by customer_id (Exchange enforcer)
--   Cost: transfer 80% of order data across network
-- Option B: Broadcast customers to all nodes (smaller table)
--   Cost: replicate 100K customer rows to N nodes
-- Optimizer compares enforcer costs for each distribution strategy
```

### Positive: Merge join with sort enforcers

```sql
SELECT * FROM orders o
JOIN lineitem l ON o.order_id = l.order_id
ORDER BY o.order_id;

-- MergeJoin needs both inputs sorted on order_id
-- If orders has clustered index: no enforcer needed for left
-- If lineitem not sorted: Sort enforcer on right
-- Output naturally sorted: no enforcer for ORDER BY
-- 2 of 3 potential enforcers eliminated
```

## References

**Original papers:**
- Graefe, G., McKenna, W.J., "The Volcano Optimizer Generator: Extensibility and Efficient Search", IEEE Data Engineering 1993
  - DOI: 10.1109/69.273032
  - Section 3.3: "Enforcers" -- Sort and Exchange operators
  - "An enforcer is a physical operator that does not correspond to any
    logical operator but ensures a physical property"

- Graefe, G., "The Cascades Framework for Query Optimization", IEEE Data Engineering Bulletin 1995
  - DOI: 10.1109/69.469815
  - Extended enforcer framework with property derivation

**Detailed analysis:**
- Simmen, D.E., Shekita, E.J., Malkemus, T., "Fundamental Techniques for Order Optimization", ACM SIGMOD 1996
  - DOI: 10.1145/233269.233320
  - Formal treatment of sort enforcers and order propagation

**Implementation in databases:**
- mssql: Sort and Exchange enforcers in Cascades optimizer
- CockroachDB: `pkg/sql/opt/xform/physical_props.go` - enforcer insertion
- Greenplum Orca: `libgpopt/src/operators/CPhysicalSort.cpp` - sort enforcer
- PostgreSQL: Sort node insertion in `createplan.c`
