# Rule: Neo4j OPTIONAL MATCH to AntiSemiApply

**Category:** database-specific/neo4j
**File:** `rules/database-specific/neo4j/optional-match-to-anti-semi-apply.rra`

## Metadata

- **ID:** `neo4j-optional-match-to-anti-semi-apply`
- **Version:** "1.0.0"
- **Databases:** neo4j
- **Tags:** optional-match, anti-semi-apply, null-check, cypher, optimization
- **Authors:** "Neo4j Inc."


# Neo4j OPTIONAL MATCH to AntiSemiApply

## Description

Rewrites `OPTIONAL MATCH ... WHERE x IS NULL` patterns into AntiSemiApply
operators that short-circuit on the first match. This pattern (checking for
non-existence) is more efficient as an anti-semi-join because it doesn't need
to enumerate all optional matches -- it only needs to determine if any exist.

**When to apply**: Cypher queries using `OPTIONAL MATCH` followed by a `WHERE`
clause checking that the optional variable `IS NULL`, which is the idiomatic
Cypher pattern for "NOT EXISTS".

**Why it works**: `OPTIONAL MATCH` produces rows with NULL for non-matching
patterns. Filtering for `IS NULL` selects only rows where the pattern didn't
match (anti-join semantics). AntiSemiApply stops at the first match per outer
row (short-circuit), avoiding the cost of enumerating all matches that would
be filtered out anyway.

## Relational Algebra

```algebra
-- Before: full optional match + null filter
sigma[m IS NULL](
  outer-join(label-scan(:Person, n),
             expand(n, :KNOWS, m)))

-- After: anti-semi-apply (short-circuits on first match)
anti-semi-apply(
  label-scan(:Person, n),
  expand(n, :KNOWS))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("neo4j-optional-match-null-to-anti-semi";
    "(filter (is-null ?opt-var)
       (optional-expand ?source ?rel-type ?dir ?opt-var))" =>
    "(anti-semi-apply ?source
       (expand-exists ?source ?rel-type ?dir))"
),

rw!("neo4j-optional-match-not-null-to-semi";
    "(filter (is-not-null ?opt-var)
       (optional-expand ?source ?rel-type ?dir ?opt-var))" =>
    "(semi-apply ?source
       (expand-exists ?source ?rel-type ?dir))"
),
```

## Preconditions

```rust
fn applicable(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> bool {
    stats.has_optional_match
        && stats.has_null_check_on_optional_var
        && stats.avg_degree > 1.0
}
```

**Restrictions:**
- Only applies when the optional variable is only used in the IS NULL check
- If the optional variable's properties are accessed elsewhere, must keep OPTIONAL MATCH
- Pattern must be a simple expansion (single hop); multi-hop requires different handling
- Cannot apply when optional match has additional filter predicates on the optional variable

## Cost Model

```rust
fn estimated_benefit(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> f64 {
    let outer_rows = stats.outer_cardinality as f64;
    let avg_degree = stats.avg_degree as f64;
    let match_probability = stats.optional_match_probability;

    // Optional match: enumerate all matches, then filter
    let optional_cost = outer_rows * avg_degree * 0.001;

    // Anti-semi-apply: short-circuit on first match
    let anti_semi_cost = outer_rows * (
        match_probability * 1.0 * 0.001  // found: stop at first
        + (1.0 - match_probability) * avg_degree * 0.001  // not found: check all
    );

    if optional_cost > anti_semi_cost {
        (optional_cost - anti_semi_cost) / optional_cost
    } else {
        0.0
    }
}
```

**Typical benefit**: 20% to 5x for high-degree nodes where most have matches.

## Test Cases

### Positive: NOT EXISTS pattern

```cypher
// Find people who don't know anyone
MATCH (p:Person)
OPTIONAL MATCH (p)-[:KNOWS]->(friend)
WHERE friend IS NULL
RETURN p.name

// Rewritten to:
// MATCH (p:Person)
// WHERE NOT EXISTS { (p)-[:KNOWS]->() }
// RETURN p.name
// EXPLAIN shows: AntiSemiApply instead of OptionalExpand + Filter
```

### Positive: Semi-apply for existence check

```cypher
// Find people who know at least one person (existence check)
MATCH (p:Person)
OPTIONAL MATCH (p)-[:KNOWS]->(friend)
WHERE friend IS NOT NULL
RETURN DISTINCT p.name

// Rewritten to SemiApply; stops at first KNOWS relationship
```

### Negative: Optional variable properties used

```cypher
// Cannot rewrite: friend.name is accessed
MATCH (p:Person)
OPTIONAL MATCH (p)-[:KNOWS]->(friend:Person)
WHERE friend IS NULL OR friend.age > 30
RETURN p.name, friend.name

// friend variable is used beyond null check
// Must keep OptionalExpand to produce friend rows
```

## References

**Implementation:**
- Neo4j source: `org.neo4j.cypher.internal.compiler.planner.logical.steps.SelectPatternPredicates`
- Anti-semi-apply: `org.neo4j.cypher.internal.logical.plans.AntiSemiApply`
- Optional match planning: `org.neo4j.cypher.internal.compiler.planner.logical.OptionalMatchPlanner`

**Documentation:**
- Neo4j Manual: "OPTIONAL MATCH"
  - https://neo4j.com/docs/cypher-manual/current/clauses/optional-match/

**Papers:**
- Green, A., et al., "Updating Graph Databases with Cypher", 2019
