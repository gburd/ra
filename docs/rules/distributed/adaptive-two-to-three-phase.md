# Rule: Adaptive Two-to-Three-Phase Upgrade

**Category:** distributed/aggregation
**File:** `rules/distributed/aggregation/adaptive-two-to-three-phase.rra`

## Metadata

- **ID:** `adaptive-two-to-three-phase`
- **Version:** "1.0.0"
- **Databases:** spark, presto, trino
- **Tags:** distributed, aggregation, adaptive, runtime, skew-detection
- **Authors:** "RA Contributors"


# Adaptive Two-to-Three-Phase Upgrade

## Description

At runtime, monitor the partial aggregation reduction ratio. If local
pre-aggregation provides less than a threshold reduction (e.g., <20%
fewer rows), upgrade from two-phase to three-phase by inserting an
additional shuffle step. This handles cases where skew or high
cardinality was not detected at plan time.

**When to apply**: Runtime monitoring shows poor reduction ratio in
the local aggregation phase, indicating that two-phase is not
effective for the actual data distribution.

## Relational Algebra

```algebra
-- Two-phase (initial plan)
gamma[g, merge_agg(partial)](
    Exchange[hash(g)](
        gamma[g, partial_agg(a)](R)
    )
)

-- Upgraded to three-phase at runtime
gamma[g, merge_agg(partial_2)](
    Exchange[hash(g)](
        gamma[g, merge_agg(partial_1)](
            Exchange[hash(salt(g))](
                gamma[g, partial_agg(a)](R)
            )
        )
    )
)
```

## Test Cases

```sql
-- Runtime scenario: expected low cardinality, got high
SELECT session_id, SUM(duration) FROM events GROUP BY session_id;
-- Statistics said 1K sessions, actual is 1M -> upgrade to 3-phase

-- No upgrade needed: reduction ratio is good
SELECT country, COUNT(*) FROM users GROUP BY country;
-- 200 countries with 100M users -> 99.9% reduction, stay 2-phase
```

## References

Spark AQE: runtime plan adaptation
