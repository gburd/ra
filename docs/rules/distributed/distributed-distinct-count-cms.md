# Rule: Distributed Distinct Count via Count-Min Sketch

**Category:** distributed/aggregation
**File:** `rules/distributed/aggregation/distributed-distinct-count-cms.rra`

## Metadata

- **ID:** `distributed-distinct-count-cms`
- **Version:** "1.0.0"
- **Databases:** presto, trino, spark
- **Tags:** distributed, aggregation, distinct, count-min-sketch, approximate, frequency
- **Authors:** "RA Contributors"


# Distributed Distinct Count via Count-Min Sketch

## Description

Use Count-Min Sketch (CMS) for approximate frequency estimation in
distributed aggregation. CMS provides point-query frequency estimates
that can be merged across nodes. Useful for heavy hitter detection
and approximate GROUP BY with frequency thresholds.

**When to apply**: Query needs frequency estimation rather than exact
counts, or when detecting heavy hitters (values above a frequency
threshold) in a distributed setting.

## Relational Algebra

```algebra
-- Local CMS + global merge
gamma[g, cms_merge(partial_cms)](
    Exchange[hash(g)](
        gamma[g, cms_add(x) as partial_cms](R)
    )
)
```

## Test Cases

```sql
-- Positive: heavy hitter detection
SELECT url, APPROX_COUNT(url) AS freq
FROM access_logs GROUP BY url HAVING freq > 10000;

-- Positive: approximate frequency for top-K
SELECT product_id, APPROX_COUNT(product_id) AS sales_count
FROM orders GROUP BY product_id ORDER BY sales_count DESC LIMIT 100;
```

## References

Cormode and Muthukrishnan, "An Improved Data Stream Summary: The Count-Min Sketch and its Applications" (2005)
