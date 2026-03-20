# PostgreSQL Integration

This document covers RA's integration with PostgreSQL for metadata
extraction, plan comparison, and rule validation.

## Status

PostgreSQL integration is under active development.

## Planned Features

### Metadata Extraction

The `ra-metadata` crate connects to PostgreSQL to extract:

- Schema information from `pg_class`, `pg_attribute`
- Column statistics from `pg_stats`
- Index information from `pg_index`
- Constraint information from `pg_constraint`

### EXPLAIN Plan Parsing

Parse PostgreSQL's JSON EXPLAIN output to compare RA-optimized plans
against PostgreSQL's native optimizer decisions.

### Differential Testing

Run the same queries through both RA and PostgreSQL, comparing:

- Result correctness
- Plan cost estimates
- Actual execution time

## Configuration

Connection details are configured via environment variables or the
`ra-config` crate:

```bash
export RA_PG_HOST=localhost
export RA_PG_PORT=5432
export RA_PG_DATABASE=mydb
export RA_PG_USER=myuser
```

## Further Reading

- [Database Adapters](database-adapters.md) -- General database
  integration architecture
- [Dialect Translation](../guides/dialect-translation.md) --
  PostgreSQL SQL dialect support
