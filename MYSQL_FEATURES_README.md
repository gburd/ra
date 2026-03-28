# MySQL/MariaDB Features Documentation

This directory contains comprehensive documentation on MySQL/MariaDB-specific features and their support status in the Ra query optimizer.

## Documents Overview

### 1. [MYSQL_MARIADB_FEATURES_SUMMARY.md](./MYSQL_MARIADB_FEATURES_SUMMARY.md)
**Executive Summary** - Quick overview for decision makers

**Contents**:
- Current support status (✅ Supported, ⚠️ Partial, ❌ Not Supported)
- Feature comparison matrix (MySQL 5.7, 8.0, MariaDB 10.3+)
- Implementation roadmap with priorities
- Key optimization opportunities
- Performance impact estimates

**Target Audience**: Product managers, architects, stakeholders

**Reading Time**: 10-15 minutes

**Use When**:
- Planning feature prioritization
- Assessing Ra's MySQL coverage
- Estimating ROI for new features

---

### 2. [MYSQL_MARIADB_UNSUPPORTED_FEATURES.md](./MYSQL_MARIADB_UNSUPPORTED_FEATURES.md)
**Detailed Analysis** - Comprehensive technical documentation

**Contents**:
- 15 major feature categories with detailed descriptions
- Syntax examples and use cases
- MySQL vs MariaDB differences
- Implementation complexity estimates
- Optimization opportunities for each feature
- Recommended implementation phases

**Target Audience**: Engineers, technical leads, contributors

**Reading Time**: 45-60 minutes

**Use When**:
- Understanding specific feature requirements
- Planning implementation work
- Researching MySQL/MariaDB capabilities

---

### 3. [MYSQL_IMPLEMENTATION_GUIDE.md](./MYSQL_IMPLEMENTATION_GUIDE.md)
**Implementation Guide** - Step-by-step developer guide

**Contents**:
- Implementation pattern and workflow
- Code architecture and directory structure
- Complete walkthrough: Adding full-text search support
- Feature-specific implementation guides
- Testing strategy and performance validation
- Common pitfalls and solutions

**Target Audience**: Ra contributors, developers implementing features

**Reading Time**: 60-90 minutes (includes hands-on examples)

**Use When**:
- Implementing a new MySQL feature
- Reviewing code architecture
- Writing tests for MySQL features

---

## Feature Coverage Summary

### Current Status (26 Rules Implemented)

**✅ Fully Supported**:
- Window functions
- Common Table Expressions (CTEs)
- Partitioning with pruning
- Standard indexes (B-tree, hash)
- Join algorithms (hash, nested loop, batched key access)
- Subquery optimization
- Invisible indexes

**⚠️ Partially Supported**:
- Spatial/GIS functions (generic rules exist, MySQL-specific optimizations missing)
- Generated columns (some rules reference them)
- INTERSECT/EXCEPT (parser issue)
- CHECK constraints (metadata only)

**❌ Not Supported**:
- Full-text search (MATCH...AGAINST)
- JSON functions (40+ functions)
- Sequences (MariaDB-only)
- Temporal tables (MariaDB system-versioned)
- Storage engine hints
- Index/optimizer hints

### Coverage Metrics

| Metric | Value | Notes |
|--------|-------|-------|
| **Total MySQL/MariaDB Features** | ~60 | Major feature categories |
| **Currently Supported** | 26 rules | ~40-50% coverage |
| **High-Priority Missing** | 3 features | JSON, Full-Text, Generated Columns |
| **Phase 1 Target Coverage** | 60-70% | After JSON + Full-Text + Generated Cols |
| **Full Coverage Potential** | 90-95% | After all 4 phases |

---

## Quick Start

### For Decision Makers
1. Read [MYSQL_MARIADB_FEATURES_SUMMARY.md](./MYSQL_MARIADB_FEATURES_SUMMARY.md) (15 min)
2. Review implementation roadmap
3. Assess ROI based on workload analysis

### For Architects/Technical Leads
1. Read [MYSQL_MARIADB_FEATURES_SUMMARY.md](./MYSQL_MARIADB_FEATURES_SUMMARY.md) (15 min)
2. Skim [MYSQL_MARIADB_UNSUPPORTED_FEATURES.md](./MYSQL_MARIADB_UNSUPPORTED_FEATURES.md) (30 min)
3. Focus on features relevant to your use case
4. Review optimization opportunities

### For Developers/Contributors
1. Read [MYSQL_MARIADB_FEATURES_SUMMARY.md](./MYSQL_MARIADB_FEATURES_SUMMARY.md) (15 min)
2. Read relevant sections of [MYSQL_MARIADB_UNSUPPORTED_FEATURES.md](./MYSQL_MARIADB_UNSUPPORTED_FEATURES.md) (30 min)
3. Follow [MYSQL_IMPLEMENTATION_GUIDE.md](./MYSQL_IMPLEMENTATION_GUIDE.md) for implementation (90 min)
4. Refer to existing MySQL rules in `/rules/database-specific/mysql/`

---

## Feature Priority Matrix

### High Priority (Phase 1: 6-8 weeks)
| Feature | Effort | Impact | Coverage Increase |
|---------|--------|--------|-------------------|
| **JSON Functions** | 6-8 weeks | Very High | +20-30% |
| **Full-Text Search** | 3-4 weeks | High | +10-15% |
| **Generated Columns** | 2-3 weeks | High | +5-10% |

**Phase 1 Total**: 11-15 weeks, +35-55% coverage

### Medium Priority (Phase 2: 6-8 weeks)
| Feature | Effort | Impact | Coverage Increase |
|---------|--------|--------|-------------------|
| **Spatial MySQL-Specific** | 2-3 weeks | Medium | +5% |
| **Storage Engine Awareness** | 3-4 weeks | Medium | +5% |
| **Index/Optimizer Hints** | 2-3 weeks | Medium | +5% |

**Phase 2 Total**: 7-10 weeks, +15% coverage

### Low Priority (Phase 3-4: 10-13 weeks)
| Feature | Effort | Impact | Coverage Increase |
|---------|--------|--------|-------------------|
| **Temporal Tables** | 4-6 weeks | Low-Medium | +5% |
| **Advanced Partitioning** | 2-3 weeks | Medium | +3% |
| **Sequences** | 2-3 weeks | Low | +2% |
| **CHECK Constraints** | 1-2 weeks | Low | +2% |
| **Table Value Constructors** | 1 week | Low | +1% |
| **INTERSECT/EXCEPT Fix** | 3-5 days | Low | +1% |

**Phase 3-4 Total**: 10-14 weeks, +14% coverage

---

## Implementation Roadmap

```
Phase 1 (Q1 2026): High-Impact Features
├── JSON Functions (6-8 weeks)
│   ├── Parser: JSON operators (->>, ->) and 40+ functions
│   ├── Core: JsonPath, JsonFunction expr types
│   ├── Metadata: JSON column detection, functional indexes
│   ├── Rules: 10-15 new optimization rules
│   └── Tests: Unit, integration, property-based
├── Full-Text Search (3-4 weeks)
│   ├── Parser: MATCH...AGAINST syntax
│   ├── Core: FullTextMatch expr type
│   ├── Metadata: FULLTEXT index detection
│   ├── Rules: 5-8 new optimization rules
│   └── Tests: Boolean mode, query expansion
└── Generated Columns (2-3 weeks)
    ├── Metadata: Generated column metadata
    ├── Rules: Functional index rewriting
    └── Tests: Virtual vs stored columns

Estimated Total: 11-15 weeks
Expected Coverage: 75-80%

---

Phase 2 (Q2 2026): Storage & Cost Model Enhancements
├── Spatial MySQL-Specific (2-3 weeks)
├── Storage Engine Awareness (3-4 weeks)
└── Index/Optimizer Hints (2-3 weeks)

Estimated Total: 7-10 weeks
Expected Coverage: 85-90%

---

Phase 3-4 (Q3-Q4 2026): Specialized Features
├── Temporal Tables (4-6 weeks)
├── Advanced Partitioning (2-3 weeks)
├── Sequences (2-3 weeks)
└── Completeness (2-3 weeks)

Estimated Total: 10-14 weeks
Expected Coverage: 90-95%
```

---

## Optimization Impact Examples

### Example 1: Full-Text Search (10-100x speedup)

**Before (Table Scan)**:
```sql
SELECT * FROM articles WHERE title LIKE '%mysql%' OR body LIKE '%mysql%';
-- Cost: 1,000,000 rows * 0.01 = 10,000 units
-- Time: ~5 seconds
```

**After (Full-Text Index)**:
```sql
SELECT * FROM articles WHERE MATCH(title, body) AGAINST('mysql');
-- Cost: log2(1,000,000) * 0.001 + 10,000 * 0.01 = 100.02 units
-- Time: ~50ms
-- Improvement: 99x faster
```

### Example 2: JSON Path Functional Index (10-100x speedup)

**Before (Expression Evaluation)**:
```sql
SELECT * FROM users WHERE JSON_EXTRACT(profile, '$.status') = 'active';
-- Cost: 1,000,000 rows * 0.05 (JSON parse) = 50,000 units
-- Time: ~2 seconds
```

**After (Functional Index)**:
```sql
CREATE INDEX idx_status ON users((JSON_EXTRACT(profile, '$.status')));
SELECT * FROM users WHERE JSON_EXTRACT(profile, '$.status') = 'active';
-- Cost: log2(1,000,000) * 0.001 + 10,000 * 0.01 = 100.02 units
-- Time: ~50ms
-- Improvement: 40x faster
```

### Example 3: Spatial MBR Pre-Filter (5-20x speedup)

**Before (Exact Geometry Check)**:
```sql
SELECT * FROM places WHERE ST_Contains(boundary, point);
-- Cost: 100,000 rows * 0.5 (polygon intersection) = 50,000 units
-- Time: ~1 second
```

**After (MBR Pre-Filter + Spatial Index)**:
```sql
SELECT * FROM places
WHERE MBRContains(boundary, point)  -- Cheap bbox check
  AND ST_Contains(boundary, point);  -- Expensive exact check on survivors
-- Cost: log2(100,000) * 0.001 + 1,000 * 0.5 = 500.017 units
-- Time: ~50ms
-- Improvement: 20x faster
```

---

## Testing Strategy

### Test Pyramid

```
         /\
        /  \  E2E (10%)
       /    \  - Docker MySQL containers
      /------\  - Real workload queries
     /        \  - Compare with MySQL EXPLAIN
    /   Integ  \ Integration (30%)
   /   (30%)    \  - MySQL 5.7, 8.0, MariaDB 10.3+
  /--------------\  - Feature compatibility matrix
 /                \ Unit (60%)
/     Unit (60%)   \  - Parser, rules, cost model
--------------------  - Property-based tests
```

### Test Coverage Goals

| Component | Target Coverage | Current |
|-----------|----------------|---------|
| Parser | 95% | ~85% |
| Core Types | 100% | ~90% |
| Metadata | 90% | ~80% |
| Rules | 95% (per rule) | ~90% |
| Cost Models | 85% | ~70% |

### CI Pipeline

```bash
# On every commit
1. cargo test --all-features           # Unit tests
2. cargo clippy -- -D warnings          # Linting
3. cargo fmt -- --check                 # Formatting
4. cargo bench --no-run                 # Benchmark compilation

# On PR
5. cargo test --test mysql_integration -- --ignored
   # Integration tests with Docker MySQL
6. ./benchmarks/compare_with_mysql.sh
   # Performance comparison
7. cargo run --bin ra-cli -- validate rules/
   # Rule validation

# Nightly
8. cargo bench                          # Full benchmarks
9. ./tests/regression/check_plans.sh    # Regression tests
```

---

## Contributing

### Adding a New Feature

1. **Research** (1-2 days)
   - Read MySQL/MariaDB documentation
   - Study MySQL source code
   - Test behavior in real MySQL instances

2. **Implementation** (1-8 weeks depending on complexity)
   - Follow [MYSQL_IMPLEMENTATION_GUIDE.md](./MYSQL_IMPLEMENTATION_GUIDE.md)
   - Start with parser, then core types, metadata, rules, cost model
   - Write tests alongside implementation

3. **Documentation** (2-3 days)
   - Update [MYSQL_MARIADB_UNSUPPORTED_FEATURES.md](./MYSQL_MARIADB_UNSUPPORTED_FEATURES.md)
   - Add `.rra` rule files with detailed comments
   - Update this README

4. **Review & Validation** (1 week)
   - Code review
   - Performance benchmarking against MySQL
   - Integration testing across MySQL versions

### Code Review Checklist

- [ ] Parser handles all syntax variants
- [ ] Core types are serializable and have good Display impls
- [ ] Metadata queries work across MySQL 5.7, 8.0, MariaDB 10.3+
- [ ] Rules have comprehensive preconditions
- [ ] Cost model is calibrated against real MySQL
- [ ] Unit tests cover edge cases
- [ ] Integration tests validate against real MySQL
- [ ] Documentation is complete and accurate
- [ ] Benchmarks show expected performance improvements

---

## Resources

### MySQL Documentation
- [MySQL 8.0 Reference Manual](https://dev.mysql.com/doc/refman/8.0/en/)
- [MySQL 5.7 Reference Manual](https://dev.mysql.com/doc/refman/5.7/en/)
- [MySQL Optimizer Internals](https://dev.mysql.com/doc/internals/en/optimizer.html)

### MariaDB Documentation
- [MariaDB Server Documentation](https://mariadb.com/kb/en/)
- [MariaDB Optimizer](https://mariadb.com/kb/en/optimizer/)
- [System-Versioned Tables](https://mariadb.com/kb/en/system-versioned-tables/)

### Ra Codebase
- Dialect Support: `/crates/ra-dialect/src/dialect.rs`
- MySQL Metadata: `/crates/ra-metadata/src/mysql.rs`
- MySQL Rules: `/rules/database-specific/mysql/`
- SQL Parser: `/crates/ra-parser/src/sql_to_relexpr.rs`

### Academic Papers
- "Access Path Selection in a Relational Database" (System R)
- "The Volcano Optimizer Generator: Extensibility and Efficient Search"
- "Efficiency in the Columbia Database Query Optimizer"

### MySQL Source Code
- GitHub: https://github.com/mysql/mysql-server
- Key Files:
  - `sql/sql_optimizer.cc` - Main optimizer
  - `sql/sql_select.cc` - SELECT optimization
  - `sql/opt_range.cc` - Range/index optimization
  - `sql/item_json_func.cc` - JSON functions
  - `storage/innobase/fts/` - Full-text search

---

## Contact & Support

For questions or contributions:

1. **GitHub Issues**: https://github.com/yourorg/ra/issues
2. **Discussions**: https://github.com/yourorg/ra/discussions
3. **Slack**: #ra-mysql-support

---

## License

Copyright 2026 Ra Contributors

Licensed under Apache-2.0 OR MIT. See LICENSE files for details.

---

**Last Updated**: 2026-03-28
**Document Version**: 1.0
**Ra Version**: 0.x.x
