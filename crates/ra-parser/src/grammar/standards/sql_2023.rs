//! SQL:2023 - Property Graph Queries
//!
//! SQL:2023 introduces support for property graphs, allowing SQL databases to handle
//! graph-structured data alongside traditional relational data.
//!
//! # Key Features
//!
//! ## Graph Tables
//!
//! The `GRAPH_TABLE` function transforms a property graph into a table:
//!
//! ```sql
//! SELECT *
//! FROM GRAPH_TABLE (social_network
//!   MATCH (p:Person)-[:FRIENDS_WITH]->(f:Person)
//!   WHERE p.age > 25
//!   COLUMNS (p.name AS person_name, f.name AS friend_name)
//! );
//! ```
//!
//! ## Match Patterns
//!
//! Graph patterns use ASCII art syntax similar to Cypher:
//! - `(n:Label)` - Match vertex with label
//! - `-[:TYPE]->` - Match directed edge
//! - `-[:TYPE]-` - Match undirected edge
//! - `-[:TYPE*1..5]->` - Variable-length path (1 to 5 hops)
//!
//! ## Path Queries
//!
//! ```sql
//! -- Find shortest path
//! MATCH SHORTEST (start:Person)-[:KNOWS*]->(end:Person)
//! WHERE start.id = 1 AND end.id = 100
//!
//! -- Find all paths
//! MATCH ANY SHORTEST (a)-[:CONNECTED_TO*]->(b)
//!
//! -- Trail (no repeated edges)
//! MATCH TRAIL (n)-[e:LINK*3..5]->(m)
//! ```
//!
//! # References
//!
//! - ISO/IEC 9075-16:2023 - SQL/PGQ (Property Graph Queries)
//! - [SQL:2023 Wikipedia](https://en.wikipedia.org/wiki/SQL:2023)

use sqlparser::ast::Statement;
use std::error::Error;

use crate::grammar::extension::GrammarExtension;

/// SQL:2023 Property Graph Queries extension.
pub struct SQL2023Extension;

impl GrammarExtension for SQL2023Extension {
    fn name(&self) -> &str {
        "sql:2023"
    }

    fn keywords(&self) -> Vec<&str> {
        vec![
            // Graph table function
            "GRAPH_TABLE",
            "GRAPH",
            // Match patterns
            "MATCH",
            "VERTEX",
            "EDGE",
            "PATH",
            // Pattern quantifiers
            "ANY",
            "SHORTEST",
            "TRAIL",
            "ACYCLIC",
            "SIMPLE",
            // Graph elements
            "VERTICES",
            "EDGES",
            "LABELS",
            "PROPERTIES",
            // Path navigation
            "ELEMENT_ID",
            "IS_LABELED",
            "LABEL_OF",
            "PROPERTY_EXISTS",
        ]
    }

    fn operators(&self) -> Vec<&str> {
        vec![
            // Pattern operators (represented as strings in SQL)
            "->", // Directed edge (right)
            "<-", // Directed edge (left)
            "-",  // Undirected edge
            "~>", // Directed path (right)
            "<~", // Directed path (left)
            "~",  // Undirected path
        ]
    }

    fn functions(&self) -> Vec<&str> {
        vec![
            // Graph table function
            "GRAPH_TABLE",
            // Path functions
            "PATH_LENGTH",
            "VERTICES_OF_PATH",
            "EDGES_OF_PATH",
            "IS_TRAIL",
            "IS_ACYCLIC",
            "IS_SIMPLE",
            // Property access
            "PROPERTY_VALUE",
            "VERTEX_LABELS",
            "EDGE_LABEL",
        ]
    }

    fn parse_statement(&self, _sql: &str) -> Result<Option<Statement>, Box<dyn Error>> {
        // TODO: Implement GRAPH_TABLE parsing
        // For now, return None to let other parsers try
        Ok(None)
    }

    fn documentation_url(&self) -> Option<&str> {
        Some("https://en.wikipedia.org/wiki/SQL:2023")
    }

    fn min_version(&self) -> Option<&str> {
        Some("SQL:2023")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sql_2023_extension() {
        let ext = SQL2023Extension;
        assert_eq!(ext.name(), "sql:2023");

        // Check for key graph keywords
        let keywords = ext.keywords();
        assert!(keywords.contains(&"GRAPH_TABLE"));
        assert!(keywords.contains(&"MATCH"));
        assert!(keywords.contains(&"VERTEX"));
        assert!(keywords.contains(&"EDGE"));
    }

    #[test]
    fn test_graph_operators() {
        let ext = SQL2023Extension;
        let operators = ext.operators();

        assert!(operators.contains(&"->")); // Directed edge
        assert!(operators.contains(&"<-")); // Reverse directed edge
        assert!(operators.contains(&"-")); // Undirected edge
    }

    #[test]
    fn test_path_functions() {
        let ext = SQL2023Extension;
        let functions = ext.functions();

        assert!(functions.contains(&"PATH_LENGTH"));
        assert!(functions.contains(&"VERTICES_OF_PATH"));
        assert!(functions.contains(&"IS_TRAIL"));
    }
}
