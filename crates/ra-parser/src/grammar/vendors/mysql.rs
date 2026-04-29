//! MySQL-specific SQL grammar extensions.
//!
//! `MySQL` has several unique syntax features including backtick identifiers,
//! alternative LIMIT syntax, and MySQL-specific functions.
//!
//! # Key Features
//!
//! ## Backtick Identifiers
//!
//! ```sql
//! SELECT `user`.`name` FROM `users` AS `user`;
//! SELECT * FROM `table-name-with-dashes`;
//! ```
//!
//! ## LIMIT Syntax
//!
//! `MySQL` supports an alternative LIMIT syntax:
//! ```sql
//! -- Standard: LIMIT count OFFSET offset
//! SELECT * FROM users LIMIT 10 OFFSET 20;
//!
//! -- MySQL alternative: LIMIT offset, count
//! SELECT * FROM users LIMIT 20, 10;
//! ```
//!
//! ## INSERT...ON DUPLICATE KEY UPDATE
//!
//! ```sql
//! INSERT INTO users (id, name, email)
//! VALUES (1, 'Alice', 'alice@example.com')
//! ON DUPLICATE KEY UPDATE
//!   name = VALUES(name),
//!   email = VALUES(email);
//! ```
//!
//! ## MySQL-Specific Functions
//!
//! ```sql
//! SELECT GROUP_CONCAT(name SEPARATOR ', ') FROM users;
//! SELECT CONCAT_WS('|', col1, col2, col3);
//! SELECT DATE_ADD(NOW(), INTERVAL 1 DAY);
//! ```

use sqlparser::ast::Statement;
use std::error::Error;

use crate::grammar::extension::GrammarExtension;

/// MySQL-specific extension.
pub struct MySQLExtension;

impl GrammarExtension for MySQLExtension {
    fn name(&self) -> &'static str {
        "mysql"
    }

    fn keywords(&self) -> Vec<&str> {
        vec![
            // UPSERT
            "ON DUPLICATE KEY UPDATE",
            // INSERT variants
            "INSERT IGNORE",
            "REPLACE INTO",
            // SHOW statements
            "SHOW",
            "DATABASES",
            "TABLES",
            "COLUMNS",
            "STATUS",
            "VARIABLES",
            "PROCESSLIST",
            "GRANTS",
            "ENGINE",
            "ENGINES",
            "PLUGINS",
            // MySQL-specific DDL
            "AUTO_INCREMENT",
            "UNSIGNED",
            "ZEROFILL",
            // Storage engines
            "ENGINE",
            "INNODB",
            "MYISAM",
            "MEMORY",
            // Index hints
            "USE INDEX",
            "FORCE INDEX",
            "IGNORE INDEX",
            // STRAIGHT_JOIN
            "STRAIGHT_JOIN",
            // PARTITION
            "PARTITION",
            "PARTITIONS",
            "SUBPARTITION",
            // EXPLAIN
            "EXPLAIN",
            "DESCRIBE",
            "DESC",
            // SET
            "SET",
            "GLOBAL",
            "SESSION",
            "LOCAL",
            // HANDLER
            "HANDLER",
            "OPEN",
            "READ",
            "CLOSE",
            // Full-Text Search
            "MATCH",
            "AGAINST",
            "NATURAL",
            "LANGUAGE",
            "BOOLEAN",
            "EXPANSION",
            "MODE",
            "WITH QUERY EXPANSION",
        ]
    }

    fn operators(&self) -> Vec<&str> {
        vec![
            // MySQL uses standard operators plus:
            "DIV", // Integer division
            "MOD", // Modulo (also %)
            "<=>", // NULL-safe equal
            "REGEXP",
            "RLIKE",       // Regular expression
            "SOUNDS LIKE", // Phonetic comparison
        ]
    }

    fn functions(&self) -> Vec<&str> {
        vec![
            // String aggregation
            "GROUP_CONCAT",
            // String functions
            "CONCAT_WS",
            "FORMAT",
            "SUBSTRING_INDEX",
            "LOCATE",
            "INSTR",
            "LEFT",
            "RIGHT",
            "REVERSE",
            "REPEAT",
            "SPACE",
            "STRCMP",
            // Date functions
            "DATE_ADD",
            "DATE_SUB",
            "DATE_FORMAT",
            "STR_TO_DATE",
            "UNIX_TIMESTAMP",
            "FROM_UNIXTIME",
            "CURDATE",
            "CURTIME",
            "DATEDIFF",
            "TIMEDIFF",
            "TIMESTAMPDIFF",
            "TIMESTAMPADD",
            "LAST_DAY",
            "MAKEDATE",
            "MAKETIME",
            // JSON functions (MySQL 5.7+)
            "JSON_EXTRACT",
            "JSON_SET",
            "JSON_INSERT",
            "JSON_REPLACE",
            "JSON_REMOVE",
            "JSON_ARRAY",
            "JSON_OBJECT",
            "JSON_MERGE",
            "JSON_MERGE_PRESERVE",
            "JSON_UNQUOTE",
            "JSON_QUOTE",
            "JSON_TYPE",
            "JSON_VALID",
            "JSON_KEYS",
            "JSON_LENGTH",
            "JSON_DEPTH",
            "JSON_SEARCH",
            "JSON_ARRAYAGG",
            "JSON_OBJECTAGG",
            // Control flow
            "IF",
            "IFNULL",
            "NULLIF",
            "CASE",
            // Conversion functions
            "CAST",
            "CONVERT",
            "BINARY",
            // Encryption/hashing
            "MD5",
            "SHA1",
            "SHA2",
            "AES_ENCRYPT",
            "AES_DECRYPT",
            "PASSWORD",
            "ENCRYPT",
            // Math functions
            "RAND",
            "ROUND",
            "TRUNCATE",
            "CEILING",
            "FLOOR",
            "POW",
            "POWER",
            "SQRT",
            "EXP",
            "LN",
            "LOG",
            "LOG10",
            "LOG2",
            // System functions
            "DATABASE",
            "USER",
            "VERSION",
            "CONNECTION_ID",
            "LAST_INSERT_ID",
            "ROW_COUNT",
            "FOUND_ROWS",
            // Window functions (MySQL 8.0+)
            "ROW_NUMBER",
            "RANK",
            "DENSE_RANK",
            "PERCENT_RANK",
            "CUME_DIST",
            "NTILE",
            "LAG",
            "LEAD",
            "FIRST_VALUE",
            "LAST_VALUE",
            "NTH_VALUE",
        ]
    }

    fn parse_statement(&self, _sql: &str) -> Result<Option<Statement>, Box<dyn Error>> {
        // MySQL-specific statements not yet implemented
        Ok(None)
    }

    fn documentation_url(&self) -> Option<&str> {
        Some("https://dev.mysql.com/doc/refman/8.4/en/")
    }

    fn min_version(&self) -> Option<&str> {
        Some("5.7")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mysql_extension() {
        let ext = MySQLExtension;
        assert_eq!(ext.name(), "mysql");

        // Check MySQL-specific keywords
        let keywords = ext.keywords();
        assert!(keywords.contains(&"ON DUPLICATE KEY UPDATE"));
        assert!(keywords.contains(&"SHOW"));
        assert!(keywords.contains(&"AUTO_INCREMENT"));
    }

    #[test]
    fn test_group_concat() {
        let ext = MySQLExtension;
        let functions = ext.functions();
        assert!(functions.contains(&"GROUP_CONCAT"));
    }

    #[test]
    fn test_json_functions() {
        let ext = MySQLExtension;
        let functions = ext.functions();

        assert!(functions.contains(&"JSON_EXTRACT"));
        assert!(functions.contains(&"JSON_SET"));
        assert!(functions.contains(&"JSON_ARRAYAGG"));
    }
}
