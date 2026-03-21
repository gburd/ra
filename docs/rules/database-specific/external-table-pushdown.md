# Rule: External Table Pushdown (Greenplum)

**Category:** database-specific/greenplum
**File:** `rules/database-specific/greenplum/external-table-pushdown.rra`

## Metadata

- **ID:** `greenplum-external-table-pushdown`
- **Version:** "1.0.0"
- **Databases:** greenplum
- **Tags:** database-specific
- **Authors:** "RA Contributors"


# External Table Pushdown (Greenplum)

## Metadata
- **Rule ID**: `greenplum-external-table-pushdown`
- **Category**: Database-Specific / Greenplum
- **Source**: Greenplum

## Description

Greenplum pushes filters and projections to external tables (HDFS, S3, etc.) using PXF (Platform Extension Framework), reducing data transfer.

## Tags
`database-specific`, `greenplum`, `external-table`, `pushdown`, `pxf`
