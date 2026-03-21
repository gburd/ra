# RA Optimizer Research

This directory contains research notes on query optimization techniques, papers, and database system internals.

## Directory Structure

- `video-notes/` - Notes from CMU Database Group lectures and talks
- `papers/` - Summaries of academic papers on query optimization
- `optimization-rules/` - Documented optimization rules and techniques
- `postgres-internals/` - PostgreSQL planner internals research
- `gap-analysis/` - Analysis of missing optimizations in RA

## Research Sources

### Primary Sources
1. **CMU 15-445/645 Database Systems** - Andy Pavlo's lecture series
2. **CMU Database Group YouTube Channel** - Seminars and talks
3. **PostgreSQL Source Code** - planner/, optimizer/ directories
4. **Academic Papers** - VLDB, SIGMOD, ICDE conference proceedings
5. **Database Books** - Database System Concepts, Database Internals, etc.

### Key Topics
- Query rewriting and normalization
- Join ordering and enumeration
- Cost estimation and statistics
- Predicate pushdown and pruning
- Subquery decorrelation
- Index selection and covering indexes
- Parallel query execution
- Adaptive query processing
- Cardinality estimation improvements

## Extracting Optimization Rules

When documenting a new optimization rule:

1. **Create a note** in `optimization-rules/RULE_NAME.md`
2. **Include**:
   - Rule name and description
   - Pattern: before → after transformation
   - Preconditions for applicability
   - Cost impact
   - Examples from real queries
   - References (paper, lecture, system)
3. **Check if RA implements it**:
   ```bash
   grep -r "rule_name" rules/
   ```
4. **If missing**: Create an RFC in `rfcs/text/` for implementation

## Research Progress

See `gap-analysis/` for current status of research extraction and missing optimizations.

## Contributing Research Notes

Format:
```markdown
# [Source] - [Title]

**Date**: YYYY-MM-DD
**Source**: [CMU 15-445 Lecture 13 | Paper Citation | PostgreSQL src/backend/optimizer/...]

## Key Concepts

- Bullet points of main ideas

## Optimization Techniques

### Technique Name

**Description**: ...
**Applicability**: ...
**Cost Model**: ...
**Example**:
\`\`\`sql
-- Before
SELECT ...
-- After
SELECT ...
\`\`\`

## References

- Links to videos, papers, code
```
