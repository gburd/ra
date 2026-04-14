# Rules Overview

Ra includes **1,327+ optimization rules** spanning decades of database research and production system optimizations.

## Quick Navigation

- 📋 [**Rule Index**](./rule-index) - Complete alphabetical list of all rules
- 🏷️ [**By Category**](./by-category) - Rules organized by optimization type
- 🗄️ [**By Database**](./by-database) - Database-specific implementations
- 🔗 [**Dependency Graph**](./dependency-graph) - Rule interaction visualization
- 📚 [**References**](./references) - Academic papers and documentation

## Rule Categories

### **Core Optimizations**
- **[Logical Rules](./logical/)** - Algebraic transformations and rewriting
- **[Physical Rules](./physical/)** - Execution strategy selection
- **[Cost Models](./cost-models/)** - Cardinality estimation and costing

### **Advanced Features**
- **[Distributed Rules](./distributed/)** - Multi-node query optimization
- **[Hardware Rules](./hardware/)** - Platform-specific optimizations
- **[Experimental Rules](./experimental/)** - Research prototypes and ML-based rules

### **Database-Specific**
- **[PostgreSQL](./database-specific/postgresql/)** - PostgreSQL-optimized rules
- **[MySQL](./database-specific/mysql/)** - MySQL-specific transformations
- **[ClickHouse](./database-specific/clickhouse/)** - OLAP-focused optimizations
- **[DuckDB](./database-specific/duckdb/)** - Analytics engine rules

## Browse Rules

You can explore rules through:

1. **Interactive Sidebar** - Navigate through the rule tree in the left panel
2. **Search** - Use the search bar to find specific rules by name or keyword
3. **Category Pages** - Browse rules grouped by functionality
4. **Direct Links** - Access individual rule files (`.rra` format) or documentation (`.html`)

## Rule Format

Each rule includes:
- **Description** - What the optimization does
- **Algebra** - Mathematical transformation definition
- **Implementation** - Code patterns and examples
- **Tests** - Verification cases
- **References** - Academic sources and papers

## Getting Started

Start with the **[Rule Index](./rule-index)** for a complete overview, or jump into specific categories that match your use case.