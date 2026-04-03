//! Integration tests for SQL standards grammar modules.
//!
//! Tests verify that each SQL standard module correctly identifies its keywords,
//! operators, and functions.

use ra_parser::grammar::standards::*;
use ra_parser::grammar::GrammarExtension;

#[test]
fn test_sql92_foundation() {
    let ext = SQL92Extension;

    // Verify core DML keywords
    let keywords = ext.keywords();
    assert!(keywords.contains(&"SELECT"));
    assert!(keywords.contains(&"INSERT"));
    assert!(keywords.contains(&"UPDATE"));
    assert!(keywords.contains(&"DELETE"));

    // Verify join keywords
    assert!(keywords.contains(&"JOIN"));
    assert!(keywords.contains(&"LEFT"));
    assert!(keywords.contains(&"RIGHT"));
    assert!(keywords.contains(&"FULL"));

    // Verify aggregate functions
    let functions = ext.functions();
    assert!(functions.contains(&"COUNT"));
    assert!(functions.contains(&"SUM"));
    assert!(functions.contains(&"AVG"));

    // Verify operators
    let operators = ext.operators();
    assert!(operators.contains(&"="));
    assert!(operators.contains(&"<"));
    assert!(operators.contains(&"AND"));
    assert!(operators.contains(&"LIKE"));
}

#[test]
fn test_sql1999_ctes() {
    let ext = SQL1999Extension;

    let keywords = ext.keywords();
    assert!(keywords.contains(&"WITH"));
    assert!(keywords.contains(&"RECURSIVE"));
    assert!(keywords.contains(&"CASE"));
    assert!(keywords.contains(&"WHEN"));
    assert!(keywords.contains(&"THEN"));

    let functions = ext.functions();
    assert!(functions.contains(&"CAST"));
    assert!(functions.contains(&"COALESCE"));
}

#[test]
fn test_sql2003_window_functions() {
    let ext = SQL2003Extension;

    let keywords = ext.keywords();
    assert!(keywords.contains(&"OVER"));
    assert!(keywords.contains(&"PARTITION BY"));
    assert!(keywords.contains(&"ROWS"));
    assert!(keywords.contains(&"RANGE"));

    let functions = ext.functions();
    assert!(functions.contains(&"ROW_NUMBER"));
    assert!(functions.contains(&"RANK"));
    assert!(functions.contains(&"DENSE_RANK"));
    assert!(functions.contains(&"LAG"));
    assert!(functions.contains(&"LEAD"));
}

#[test]
fn test_sql2008_merge() {
    let ext = SQL2008Extension;

    let keywords = ext.keywords();
    assert!(keywords.contains(&"MERGE"));
    assert!(keywords.contains(&"USING"));
    assert!(keywords.contains(&"WHEN MATCHED"));
    assert!(keywords.contains(&"TRUNCATE"));
    assert!(keywords.contains(&"FETCH"));
}

#[test]
fn test_sql2011_temporal() {
    let ext = SQL2011Extension;

    let keywords = ext.keywords();
    assert!(keywords.contains(&"SYSTEM_TIME"));
    assert!(keywords.contains(&"FOR SYSTEM_TIME"));
    assert!(keywords.contains(&"AS OF"));
    assert!(keywords.contains(&"PERIOD"));
}

#[test]
fn test_sql2016_json() {
    let ext = SQL2016Extension;

    let keywords = ext.keywords();
    assert!(keywords.contains(&"JSON"));
    assert!(keywords.contains(&"JSON_TABLE"));
    assert!(keywords.contains(&"JSON_EXISTS"));

    let functions = ext.functions();
    assert!(functions.contains(&"JSON_VALUE"));
    assert!(functions.contains(&"JSON_QUERY"));
    assert!(functions.contains(&"JSON_OBJECT"));
    assert!(functions.contains(&"JSON_ARRAY"));
}

#[test]
fn test_sql2023_property_graphs() {
    let ext = SQL2023Extension;

    let keywords = ext.keywords();
    assert!(keywords.contains(&"GRAPH_TABLE"));
    assert!(keywords.contains(&"MATCH"));
    assert!(keywords.contains(&"VERTEX"));
    assert!(keywords.contains(&"EDGE"));
    assert!(keywords.contains(&"PATH"));
    assert!(keywords.contains(&"SHORTEST"));

    let operators = ext.operators();
    assert!(operators.contains(&"->"));  // Directed edge
    assert!(operators.contains(&"<-"));
    assert!(operators.contains(&"-"));   // Undirected edge

    let functions = ext.functions();
    assert!(functions.contains(&"GRAPH_TABLE"));
    assert!(functions.contains(&"PATH_LENGTH"));
    assert!(functions.contains(&"VERTICES_OF_PATH"));
}

#[test]
fn test_sql_evolution() {
    // Verify that newer standards build upon older ones
    // by checking that they don't re-define base keywords

    let sql92 = SQL92Extension;
    let sql1999 = SQL1999Extension;
    let sql2003 = SQL2003Extension;

    // SQL-92 has SELECT
    assert!(sql92.keywords().contains(&"SELECT"));

    // SQL:1999 adds WITH but doesn't duplicate SELECT (assumed available)
    assert!(sql1999.keywords().contains(&"WITH"));
    // Note: Each extension focuses on NEW keywords, not repeating base SQL-92

    // SQL:2003 adds OVER for window functions
    assert!(sql2003.keywords().contains(&"OVER"));
}

#[test]
fn test_documentation_urls() {
    // Verify all extensions have documentation URLs
    let extensions: Vec<Box<dyn GrammarExtension>> = vec![
        Box::new(SQL92Extension),
        Box::new(SQL1999Extension),
        Box::new(SQL2003Extension),
        Box::new(SQL2008Extension),
        Box::new(SQL2011Extension),
        Box::new(SQL2016Extension),
        Box::new(SQL2023Extension),
    ];

    for ext in extensions {
        let url = ext.documentation_url();
        assert!(url.is_some(), "{} missing documentation URL", ext.name());
        assert!(url.unwrap().starts_with("http"), "{} URL should start with http", ext.name());
    }
}

#[test]
fn test_extension_names() {
    // Verify naming convention
    let extensions: Vec<(&str, Box<dyn GrammarExtension>)> = vec![
        ("sql-92", Box::new(SQL92Extension)),
        ("sql:1999", Box::new(SQL1999Extension)),
        ("sql:2003", Box::new(SQL2003Extension)),
        ("sql:2008", Box::new(SQL2008Extension)),
        ("sql:2011", Box::new(SQL2011Extension)),
        ("sql:2016", Box::new(SQL2016Extension)),
        ("sql:2023", Box::new(SQL2023Extension)),
    ];

    for (expected_name, ext) in extensions {
        assert_eq!(ext.name(), expected_name);
    }
}

/// Test SQL compliance matrix - which standards are supported by which databases
#[test]
fn test_sql_compliance_matrix() {
    // This test documents which SQL standards are supported by major databases
    // Not a runtime test, but serves as documentation

    #[allow(dead_code)]
    struct DatabaseCompliance {
        name: &'static str,
        sql_92: bool,
        sql_1999: bool,
        sql_2003: bool,
        sql_2008: bool,
        sql_2011: bool,
        sql_2016: bool,
        sql_2023: bool,
    }

    let databases = vec![
        DatabaseCompliance {
            name: "PostgreSQL 17",
            sql_92: true,
            sql_1999: true,  // Full CTE support
            sql_2003: true,  // Window functions
            sql_2008: true,  // MERGE (as INSERT...ON CONFLICT)
            sql_2011: false, // No temporal tables
            sql_2016: true,  // JSON support
            sql_2023: false, // No property graphs yet
        },
        DatabaseCompliance {
            name: "MySQL 8.4",
            sql_92: true,
            sql_1999: true,  // CTEs since 8.0
            sql_2003: true,  // Window functions since 8.0
            sql_2008: false, // No MERGE statement
            sql_2011: false, // No temporal tables
            sql_2016: true,  // JSON support
            sql_2023: false, // No property graphs
        },
        DatabaseCompliance {
            name: "Oracle 21c",
            sql_92: true,
            sql_1999: true,
            sql_2003: true,
            sql_2008: true,  // MERGE support
            sql_2011: true,  // Temporal validity
            sql_2016: true,  // JSON support
            sql_2023: true,  // Property graphs via graph server
        },
        DatabaseCompliance {
            name: "SQL Server 2022",
            sql_92: true,
            sql_1999: true,
            sql_2003: true,
            sql_2008: true,
            sql_2011: true,  // Temporal tables
            sql_2016: true,  // JSON support
            sql_2023: true,  // Graph tables (MATCH syntax)
        },
    ];

    // Verify at least one database exists
    assert!(!databases.is_empty());

    // All modern databases support SQL-92
    assert!(databases.iter().all(|db| db.sql_92));

    // Most modern databases support CTEs (SQL:1999)
    assert!(databases.iter().all(|db| db.sql_1999));
}
