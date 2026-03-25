# RFC 0085: Platform and Extension-Specific Rule Organization

- Start Date: 2026-03-25
- Author: Ra Research Team
- Status: Proposed
- Tracking Issue: TBD

## Summary

Ra's optimizer must handle platform-specific features (PostgreSQL RUM indexes, Oracle JSON Duality, SQL Server XML indexes) and extension-specific behaviors (CitusDB distribution, PostGIS spatial operations, TimescaleDB hypertables). This RFC establishes architectural patterns for organizing, loading, and integrating platform-specific optimization rules in a maintainable, testable, and extensible way. The design uses a three-tier structure: (1) dialect detection, (2) conditional rule loading, and (3) platform-specific cost model overrides.

## Motivation

Ra aims to be a universal query optimizer that generates optimal plans across multiple database platforms. However, each platform has unique features that require specialized knowledge:

**PostgreSQL-specific:**
- RUM indexes (RFC 0079)
- TOAST storage (RFC 0082)
- HOT updates (RFC 0082)
- Extensions: CitusDB (RFC 0081), PostGIS, TimescaleDB, pg_partman

**Oracle-specific:**
- JSON Relational Duality views (RFC 0084)
- XMLType and XMLIndex (RFC 0083)
- Advanced Queuing
- Partitioning strategies

**SQL Server-specific:**
- XML indexes (primary, PATH, VALUE, PROPERTY) (RFC 0083)
- Columnstore indexes
- Temporal tables
- Memory-optimized tables

**MySQL-specific:**
- Full-text indexes
- Spatial indexes (MyISAM, InnoDB)
- Partitioning

Without a clear organizational structure, platform-specific rules would scatter across the codebase, making them hard to discover, test, and maintain.

## Guide-level explanation

### Three-Tier Architecture

**Tier 1: Dialect Detection**

At plan initialization, Ra detects the target dialect and queries platform metadata:

```rust
pub enum Dialect {
    PostgreSQL { version: Version, extensions: Vec<String> },
    Oracle { version: Version },
    SQLServer { version: Version },
    MySQL { version: Version, engine: String },
    // ...
}

impl Dialect {
    pub fn detect(connection: &Connection) -> Result<Self, Error> {
        // Query pg_version, v$version, @@version, etc.
    }
}
```

**Tier 2: Conditional Rule Loading**

Each platform module provides a `rules()` function that returns e-graph rewrite rules:

```rust
pub fn platform_rules(dialect: &Dialect) -> Vec<Rewrite<RelLang, RelAnalysis>> {
    let mut rules = Vec::new();

    // Always load core rules
    rules.extend(consensus_rules());

    // Platform-specific rules
    match dialect {
        Dialect::PostgreSQL { extensions, .. } => {
            rules.extend(postgresql::base_rules());

            if extensions.contains(&"rum".to_string()) {
                rules.extend(postgresql::rum_rules());
            }
            if extensions.contains(&"citus".to_string()) {
                rules.extend(postgresql::citus_rules());
            }
            if extensions.contains(&"postgis".to_string()) {
                rules.extend(postgresql::postgis_rules());
            }
        }
        Dialect::Oracle { .. } => {
            rules.extend(oracle::base_rules());
            rules.extend(oracle::json_duality_rules());
            rules.extend(oracle::xml_rules());
        }
        // ... other dialects
    }

    rules
}
```

**Tier 3: Platform-Specific Cost Models**

Each platform module can override cost parameters:

```rust
pub trait CostModel {
    fn index_scan_cost(&self, index_type: IndexType, rows: f64) -> Cost;
    fn network_transfer_cost(&self, bytes: u64) -> Cost;
    // ... other cost methods
}

pub struct PostgreSQLCostModel {
    rum_available: bool,
    citus_enabled: bool,
}

impl CostModel for PostgreSQLCostModel {
    fn index_scan_cost(&self, index_type: IndexType, rows: f64) -> Cost {
        match index_type {
            IndexType::RUM if self.rum_available => {
                // RUM-specific cost
                Cost::from_io_ops(rows * 0.01) + Cost::from_cpu_ms(rows * 0.2)
            }
            _ => default_index_scan_cost(index_type, rows),
        }
    }
}
```

### Directory Structure

Platform-specific code lives in organized module hierarchies:

```
crates/ra-engine/src/
├── platform/
│   ├── mod.rs                    # Platform detection and rule loader
│   ├── postgresql/
│   │   ├── mod.rs                # PostgreSQL base rules and cost model
│   │   ├── rum_index.rs          # RFC 0079 implementation
│   │   ├── toast.rs              # RFC 0082 TOAST cost modeling
│   │   ├── hot_updates.rs        # RFC 0082 HOT eligibility
│   │   └── extensions/
│   │       ├── mod.rs
│   │       ├── citus.rs          # RFC 0081 implementation
│   │       ├── postgis.rs        # Spatial query optimization
│   │       ├── timescaledb.rs    # Time-series optimization
│   │       └── pg_partman.rs     # Partition management awareness
│   ├── oracle/
│   │   ├── mod.rs                # Oracle base rules
│   │   ├── json_duality.rs       # RFC 0084 implementation
│   │   ├── xmltype.rs            # RFC 0083 Oracle XML
│   │   └── advanced_queueing.rs  # Oracle AQ optimization
│   ├── sqlserver/
│   │   ├── mod.rs
│   │   ├── xml_indexes.rs        # RFC 0083 SQL Server XML
│   │   ├── columnstore.rs        # Columnstore index optimization
│   │   └── temporal_tables.rs    # System-versioned tables
│   └── mysql/
│       ├── mod.rs
│       ├── fulltext.rs           # MySQL full-text indexes
│       └── spatial.rs            # MySQL spatial indexes
```

### Extension Detection

Ra queries the catalog for installed extensions and activates appropriate rules:

**PostgreSQL:**
```sql
SELECT extname, extversion
FROM pg_extension
WHERE extname IN ('citus', 'postgis', 'timescaledb', 'rum', 'pg_partman');
```

**Oracle:**
```sql
SELECT comp_name, status
FROM dba_registry
WHERE comp_name IN ('XML Database', 'Advanced Queuing', 'Spatial');
```

### Rule Registration Example

Each platform module exports its rules via a standard interface:

```rust
// crates/ra-engine/src/platform/postgresql/extensions/citus.rs

use egg::{rewrite, Rewrite};
use crate::egraph::{RelLang, RelAnalysis};

pub fn citus_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        rewrite!("citus-colocated-join";
            "(join ?left ?right ?cond)" =>
            "(citus-colocated-join ?left ?right ?cond)"
            if is_colocated_join("?left", "?right", "?cond")),

        rewrite!("citus-reference-table-broadcast-skip";
            "(join ?dist ?ref ?cond)" =>
            "(local-join ?dist ?ref ?cond)"
            if is_reference_table("?ref")),

        rewrite!("citus-distributed-agg-pushdown";
            "(agg ?group ?aggs (scan ?table))" =>
            "(citus-distributed-agg ?group ?aggs ?table)"
            if can_pushdown_agg("?group", "?table")),

        // ... more Citus-specific rules
    ]
}
```

Integration into main optimizer:

```rust
// crates/ra-engine/src/rewrite.rs

pub fn all_rules_unsorted() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    let mut rules = Vec::new();

    // Core consensus rules (always loaded)
    rules.extend(consensus_rules());

    // Platform-specific rules (conditionally loaded)
    rules.extend(platform_rules(&current_dialect()));

    rules
}
```

### Cost Model Override Example

```rust
// crates/ra-engine/src/platform/postgresql/mod.rs

pub struct PostgreSQLCostModel {
    version: Version,
    rum_available: bool,
    citus_enabled: bool,
    toast_threshold: u64,
}

impl CostModel for PostgreSQLCostModel {
    fn index_scan_cost(&self, index: &IndexInfo, selectivity: f64) -> Cost {
        let rows = index.table_rows * selectivity;

        match &index.index_type {
            IndexType::RUM { opclass, .. } if self.rum_available => {
                rum_scan_cost(opclass, rows, index.avg_entry_width)
            }
            IndexType::GIN { .. } => {
                gin_scan_cost(rows, index.avg_entry_width)
            }
            _ => default_btree_scan_cost(rows),
        }
    }

    fn column_access_cost(&self, column: &ColumnInfo) -> Cost {
        if column.avg_width > self.toast_threshold {
            // TOAST penalty: 2x I/O
            Cost::from_io_ops(2.0)
        } else {
            Cost::from_io_ops(1.0)
        }
    }
}
```

## Reference-level explanation

### Platform Detection Algorithm

```rust
pub fn detect_platform(conn: &Connection) -> Result<PlatformContext, Error> {
    let dialect = detect_dialect(conn)?;
    let extensions = detect_extensions(conn, &dialect)?;
    let capabilities = detect_capabilities(conn, &dialect)?;

    Ok(PlatformContext {
        dialect,
        extensions,
        capabilities,
    })
}

fn detect_extensions(conn: &Connection, dialect: &Dialect) -> Result<Vec<Extension>, Error> {
    match dialect {
        Dialect::PostgreSQL { .. } => {
            let rows = conn.query(
                "SELECT extname, extversion FROM pg_extension",
                &[],
            )?;
            rows.into_iter()
                .map(|row| Extension {
                    name: row.get(0),
                    version: row.get(1),
                })
                .collect()
        }
        // ... other dialects
    }
}
```

### Rule Priority and Conflict Resolution

When multiple rules could apply, Ra uses priority ordering:

1. **Extension-specific rules** (highest priority)
   - Example: CitusDB co-located join
2. **Platform-specific rules**
   - Example: PostgreSQL RUM index scan
3. **Dialect-generic rules**
   - Example: Standard GIN index scan
4. **Consensus rules** (lowest priority)
   - Example: Generic join commutativity

Conflicts are resolved by rule specificity: more specific patterns win.

### Testing Strategy

**Unit tests per module:**
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn citus_colocated_join_detected() {
        let metadata = CitusMetadata::new(/* ... */);
        let plan = /* ... */;
        let result = optimize_with_citus(plan, &metadata);
        assert!(result.uses_colocated_join());
    }
}
```

**Integration tests:**
```rust
#[test]
fn test_postgresql_with_rum_and_citus() {
    let conn = test_connection_with_extensions(&["rum", "citus"]);
    let platform = detect_platform(&conn).unwrap();
    let rules = platform_rules(&platform.dialect);

    // Verify both RUM and Citus rules are loaded
    assert!(rules.iter().any(|r| r.name().contains("rum")));
    assert!(rules.iter().any(|r| r.name().contains("citus")));
}
```

### Performance Considerations

**Rule loading overhead:**
- Detection queries run once per session
- Rule vectors are cached per platform configuration
- Extension checks use simple string comparisons

**E-graph bloat:**
- Platform-specific rules only fire when applicable patterns exist
- Conditional application via `if` guards prevents spurious rewrites
- Cost model overrides are O(1) lookups

### Backward Compatibility

When a platform-specific rule improves an existing plan, the old plan remains in the e-graph as an alternative. The cost-based extraction ensures the optimizer picks the best plan, whether it uses platform-specific features or not.

## Drawbacks

- **Complexity**: Three-tier architecture adds indirection
- **Testing burden**: Each platform × extension combination needs tests
- **Maintenance**: Platform-specific code can diverge as databases evolve
- **Documentation**: Rules scattered across modules are harder to discover

## Rationale and alternatives

### Why This Design?

**Modularity**: Platform-specific code is isolated, making it easy to add/remove without affecting core optimizer.

**Extensibility**: New platforms or extensions slot into existing structure.

**Testability**: Each module can be tested independently with mocked platform metadata.

**Discoverability**: Clear directory structure makes it obvious where to add new platform-specific rules.

### Alternative: Monolithic Rules File

All rules in a single `rewrite.rs` file with conditional compilation:

```rust
#[cfg(feature = "postgresql")]
rules.extend(postgresql_rules());
#[cfg(feature = "oracle")]
rules.extend(oracle_rules());
```

**Rejected because:**
- Hard to navigate large file
- Cargo features don't support runtime detection
- No way to handle extensions (e.g., CitusDB on some PostgreSQL instances)

### Alternative: Plugin System

Load platform-specific rules from external `.so` files at runtime.

**Rejected because:**
- ABI stability concerns
- Security risks (untrusted plugins)
- Deployment complexity
- Overkill for built-in platform support

## Prior art

### Apache Calcite

Calcite has dialect-specific rule sets:
- `org.apache.calcite.rel.rules.CoreRules` (common)
- `org.apache.calcite.adapter.jdbc.JdbcRules` (JDBC-specific)
- `org.apache.calcite.adapter.druid.DruidRules` (Druid-specific)

Each adapter registers its rules via `RelOptPlanner.addRule()`.

### Apache Spark

Spark uses `SparkStrategy` objects that pattern-match on logical plans:
- `FileSourceStrategy`
- `DataSourceV2Strategy`
- `JoinSelection`

Platform-specific strategies register with `SparkPlanner.strategies`.

### PostgreSQL

PostgreSQL's planner has hardcoded knowledge of all index types, with extension hooks for custom AMs:
- `amcostestimate` callback for custom cost models
- `amsupport` for operator strategy selection

Ra's design is similar but more modular (extension rules are separate modules, not callbacks).

## Unresolved questions

- Should rule priority be explicit (numeric priority field) or implicit (registration order)? (Recommendation: implicit via registration order)
- How should Ra handle platform version differences (e.g., PostgreSQL 13 vs 16)? (Recommendation: version guards in rule conditionals)
- Should platform detection cache results across query sessions? (Recommendation: yes, with TTL for extension changes)

## Future possibilities

- **Dynamic rule loading**: Hot-reload platform rules without restarting
- **Rule marketplace**: Community-contributed platform-specific rules
- **Rule profiling**: Track which platform rules fire most often
- **Multi-platform optimization**: Generate plans for multiple platforms and pick the fastest

## Implementation Status

✅ Architecture defined
✅ RUM index module implemented (RFC 0079) - `rum_index.rs` (1,462 lines)
✅ CitusDB optimizer implemented (RFC 0081) - `citus_optimizer.rs`
✅ Oracle JSON Duality implemented (RFC 0084) - `oracle_json_duality.rs` (1,412 lines)
✅ XML optimizer implemented (RFC 0083) - `xml_optimizer.rs` (2,057 lines)
✅ Document algebra implemented (RFC 0082) - `document_algebra.rs` (1,880 lines)

⏳ Directory restructuring (move modules into `platform/` hierarchy)
⏳ Platform detection framework
⏳ Extension detection queries
⏳ Cost model override system

## Summary of Platform-Specific Modules

| Module | RFC | Lines | Status | Key Features |
|--------|-----|-------|--------|-------------|
| rum_index.rs | 0079 | 1,462 | ✅ | RUM index detection, distance ordering, cost model |
| citus_optimizer.rs | 0081 | ~1,400 | ✅ | Co-located joins, shard pruning, columnar storage |
| oracle_json_duality.rs | 0084 | 1,412 | ✅ | Access path selection, predicate pushdown, update cost |
| xml_optimizer.rs | 0083 | 2,057 | ✅ | XPath parsing, XML index awareness, FLWOR rewriting |
| document_algebra.rs | 0082 | 1,880 | ✅ | MongoDB formal semantics, TOAST/HOT integration |

**Total: 8,211 lines of platform-specific optimization code**

## References

1. Apache Calcite adapter architecture: https://calcite.apache.org/docs/adapter.html
2. Apache Spark custom strategies: https://spark.apache.org/docs/latest/sql-ref.html
3. PostgreSQL extensible indexes: https://www.postgresql.org/docs/current/indexam.html
4. Ra RFCs: 0079 (RUM), 0080 (DocumentDB RUM), 0081 (CitusDB), 0082 (MongoDB), 0083 (XPath/XQuery), 0084 (Oracle JSON Duality)
