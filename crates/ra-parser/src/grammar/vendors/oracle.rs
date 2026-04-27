//! Oracle-specific SQL grammar extensions.
//!
//! Oracle Database has many proprietary features including CONNECT BY hierarchical queries,
//! the DUAL table, and the (+) outer join operator.
//!
//! # Key Features
//!
//! ## CONNECT BY Hierarchical Queries
//!
//! ```sql
//! SELECT employee_id, name, manager_id, LEVEL
//! FROM employees
//! START WITH manager_id IS NULL
//! CONNECT BY PRIOR employee_id = manager_id
//! ORDER SIBLINGS BY name;
//! ```
//!
//! ## DUAL Table
//!
//! Oracle requires a FROM clause, using DUAL for expressions:
//! ```sql
//! SELECT SYSDATE FROM DUAL;
//! SELECT 1+1 FROM DUAL;
//! ```
//!
//! ## (+) Outer Join Operator (Legacy)
//!
//! ```sql
//! -- Old-style outer join
//! SELECT e.name, d.dept_name
//! FROM employees e, departments d
//! WHERE e.dept_id = d.id(+);
//! ```
//!
//! ## Sequences
//!
//! ```sql
//! SELECT emp_seq.NEXTVAL FROM DUAL;
//! INSERT INTO employees (id, name) VALUES (emp_seq.NEXTVAL, 'Alice');
//! ```

use sqlparser::ast::Statement;
use std::error::Error;

use crate::grammar::extension::GrammarExtension;

/// Oracle-specific extension.
pub struct OracleExtension;

impl GrammarExtension for OracleExtension {
    fn name(&self) -> &str {
        "oracle"
    }

    fn keywords(&self) -> Vec<&str> {
        vec![
            // Hierarchical queries
            "CONNECT BY",
            "START WITH",
            "PRIOR",
            "NOCYCLE",
            "LEVEL",
            "SYS_CONNECT_BY_PATH",
            "CONNECT_BY_ROOT",
            "ORDER SIBLINGS BY",
            // DUAL table (not really a keyword but special)
            "DUAL",
            // Sequences
            "NEXTVAL",
            "CURRVAL",
            // MERGE enhancements
            "MERGE",
            "MATCHED",
            "NOT MATCHED",
            // Flashback
            "FLASHBACK",
            "AS OF",
            "TIMESTAMP",
            "SCN",
            // MODEL clause
            "MODEL",
            "DIMENSION BY",
            "MEASURES",
            "RULES",
            // Analytic functions
            "KEEP",
            "DENSE_RANK FIRST",
            "DENSE_RANK LAST",
            // PIVOT/UNPIVOT
            "PIVOT",
            "UNPIVOT",
            "FOR",
            "IN",
            // PL/SQL integration
            "EXECUTE",
            "IMMEDIATE",
            "BULK COLLECT",
            // Autonomous transactions
            "PRAGMA",
            "AUTONOMOUS_TRANSACTION",
        ]
    }

    fn operators(&self) -> Vec<&str> {
        vec![
            // Old-style outer join
            "(+)", // String concatenation
            "||",
        ]
    }

    fn functions(&self) -> Vec<&str> {
        vec![
            // Aggregate functions
            "LISTAGG",
            "COLLECT",
            "XMLAGG",
            // String functions
            "INSTR",
            "SUBSTR",
            "REPLACE",
            "TRANSLATE",
            "TRIM",
            "NVL",
            "NVL2",
            "DECODE",
            "COALESCE",
            "INITCAP",
            "LPAD",
            "RPAD",
            // Date functions
            "TO_DATE",
            "TO_CHAR",
            "TO_TIMESTAMP",
            "TO_NUMBER",
            "TRUNC",
            "ADD_MONTHS",
            "MONTHS_BETWEEN",
            "NEXT_DAY",
            "LAST_DAY",
            "SYSDATE",
            "SYSTIMESTAMP",
            "CURRENT_DATE",
            "CURRENT_TIMESTAMP",
            "EXTRACT",
            // Conversion functions
            "CAST",
            "TO_CLOB",
            "TO_BLOB",
            "TO_NCLOB",
            // Analytic functions
            "ROW_NUMBER",
            "RANK",
            "DENSE_RANK",
            "NTILE",
            "LAG",
            "LEAD",
            "FIRST_VALUE",
            "LAST_VALUE",
            "RATIO_TO_REPORT",
            "PERCENT_RANK",
            "CUME_DIST",
            "PERCENTILE_CONT",
            "PERCENTILE_DISC",
            // JSON functions (12c+)
            "JSON_TABLE",
            "JSON_QUERY",
            "JSON_VALUE",
            "JSON_EXISTS",
            "JSON_OBJECT",
            "JSON_ARRAY",
            "JSON_ARRAYAGG",
            "JSON_OBJECTAGG",
            // Hierarchical functions
            "SYS_CONNECT_BY_PATH",
            "CONNECT_BY_ROOT",
            // XML functions
            "XMLELEMENT",
            "XMLATTRIBUTES",
            "XMLFOREST",
            "XMLAGG",
            "XMLQUERY",
            "XMLTABLE",
            "XMLEXISTS",
            // System functions
            "USER",
            "SYS_CONTEXT",
            "USERENV",
        ]
    }

    fn parse_statement(&self, _sql: &str) -> Result<Option<Statement>, Box<dyn Error>> {
        // Oracle-specific statements not yet implemented
        Ok(None)
    }

    fn documentation_url(&self) -> Option<&str> {
        Some("https://docs.oracle.com/en/database/oracle/oracle-database/21/")
    }

    fn min_version(&self) -> Option<&str> {
        Some("12c")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oracle_extension() {
        let ext = OracleExtension;
        assert_eq!(ext.name(), "oracle");

        // Check CONNECT BY
        let keywords = ext.keywords();
        assert!(keywords.contains(&"CONNECT BY"));
        assert!(keywords.contains(&"START WITH"));
        assert!(keywords.contains(&"PRIOR"));

        // Check (+) operator
        let operators = ext.operators();
        assert!(operators.contains(&"(+)"));
    }

    #[test]
    fn test_oracle_functions() {
        let ext = OracleExtension;
        let functions = ext.functions();

        assert!(functions.contains(&"LISTAGG"));
        assert!(functions.contains(&"NVL"));
        assert!(functions.contains(&"DECODE"));
        assert!(functions.contains(&"SYS_CONNECT_BY_PATH"));
    }
}
