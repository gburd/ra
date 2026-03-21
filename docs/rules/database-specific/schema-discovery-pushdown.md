# Rule: Schema Discovery Pushdown (Drill)

**Category:** database-specific/drill
**File:** `rules/database-specific/drill/schema-discovery-pushdown.rra`

## Metadata

- **ID:** `drill-schema-discovery-pushdown`
- **Version:** "1.0.0"
- **Databases:** drill
- **Tags:** database-specific
- **Authors:** "RA Contributors"


# Schema Discovery Pushdown (Drill)

## Metadata
- **Rule ID**: `drill-schema-discovery-pushdown`
- **Category**: Database-Specific / Drill
- **Source**: Apache Drill
- **Docs**: https://drill.apache.org/docs/

## Description

Drill defers schema discovery until runtime and pushes schema inference into storage plugins, enabling schema-free querying of JSON, Parquet, MongoDB, etc.

**Key innovation**: Late-binding schema allows querying heterogeneous data without DDL.

## Relational Algebra

```
Scan(schemaless_source)
→ Scan_with_schema_discovery(source, infer_at_runtime)
```

## Test Cases

### Test 1: Schemaless JSON query
```sql
-- No CREATE TABLE needed
SELECT t.name, t.address.city, t.orders[0].amount
FROM dfs.`/data/users.json` t
WHERE t.age > 25;

-- Schema discovered at runtime from JSON structure
-- Supports nested fields and arrays without schema definition
```

### Test 2: Heterogeneous schema evolution
```sql
-- Files have different schemas
-- file1.json: {id, name, email}
-- file2.json: {id, name, phone}

SELECT id, name, email, phone
FROM dfs.`/data/*.json`;

-- Drill handles schema evolution:
-- Missing columns filled with NULL
-- Union of all schemas across files
```

## References

1. **Drill Docs**: "Schema-Free SQL Query Engine"
2. **Paper**: "Drill: Interactive Analysis of Large-Scale Datasets" (SIGMOD 2013)

## Tags
`database-specific`, `drill`, `schema-free`, `late-binding`, `json`, `schemaless`
