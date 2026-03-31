//! SQL Server (T-SQL) specific grammar extensions.
//!
//! Microsoft SQL Server uses T-SQL (Transact-SQL), which includes many unique
//! features like square bracket identifiers, TOP clause, and OUTPUT clause.
//!
//! # Key Features
//!
//! ## Square Bracket Identifiers
//!
//! ```sql
//! SELECT [First Name], [Date of Birth]
//! FROM [Customer Data]
//! WHERE [Customer ID] = 123;
//! ```
//!
//! ## TOP Clause
//!
//! ```sql
//! SELECT TOP 10 * FROM users ORDER BY created_at DESC;
//! SELECT TOP 10 PERCENT * FROM sales ORDER BY amount DESC;
//! SELECT TOP 5 WITH TIES * FROM scores ORDER BY score DESC;
//! ```
//!
//! ## OUTPUT Clause
//!
//! ```sql
//! INSERT INTO users (name) OUTPUT inserted.id, inserted.created_at VALUES ('Alice');
//! UPDATE users SET active = 0 OUTPUT deleted.name, inserted.name WHERE id = 1;
//! DELETE FROM users OUTPUT deleted.* WHERE id = 1;
//! ```
//!
//! ## Graph Tables (SQL Server 2017+)
//!
//! ```sql
//! CREATE TABLE Person (id INT, name VARCHAR(100)) AS NODE;
//! CREATE TABLE Likes (rating INT) AS EDGE;
//!
//! SELECT p1.name, p2.name
//! FROM Person p1, Likes, Person p2
//! WHERE MATCH(p1-(Likes)->p2);
//! ```

use sqlparser::ast::Statement;
use std::error::Error;

use crate::grammar::extension::GrammarExtension;

/// SQL Server (T-SQL) specific extension.
pub struct SQLServerExtension;

impl GrammarExtension for SQLServerExtension {
    fn name(&self) -> &str {
        "sqlserver"
    }

    fn keywords(&self) -> Vec<&str> {
        vec![
            // TOP clause
            "TOP", "PERCENT", "WITH TIES",
            // OUTPUT clause
            "OUTPUT", "inserted", "deleted",
            // MERGE enhancements
            "MERGE", "MATCHED", "NOT MATCHED", "BY TARGET", "BY SOURCE",
            // Graph tables
            "NODE", "EDGE", "MATCH",
            // Temporal tables
            "FOR SYSTEM_TIME", "AS OF", "FROM", "TO", "BETWEEN", "CONTAINED IN",
            "PERIOD FOR SYSTEM_TIME",
            // GO batch separator
            "GO",
            // Control flow
            "IF", "ELSE", "BEGIN", "END", "WHILE", "BREAK", "CONTINUE",
            "RETURN", "WAITFOR", "DELAY", "TIME",
            // Error handling
            "TRY", "CATCH", "THROW", "RAISERROR",
            // Transactions
            "BEGIN TRANSACTION", "COMMIT TRANSACTION", "ROLLBACK TRANSACTION",
            "SAVE TRANSACTION",
            // Variables
            "DECLARE", "SET",
            // Cursors
            "CURSOR", "OPEN", "FETCH", "CLOSE", "DEALLOCATE",
            // CTEs
            "WITH", "XMLNAMESPACES",
            // PIVOT/UNPIVOT
            "PIVOT", "UNPIVOT",
            // Index hints
            "WITH", "INDEX", "NOLOCK", "READPAST", "UPDLOCK", "XLOCK",
            "ROWLOCK", "PAGLOCK", "TABLOCK", "TABLOCKX",
            "NOEXPAND", "FORCESEEK",
        ]
    }

    fn operators(&self) -> Vec<&str> {
        vec![
            // T-SQL operators
            "+=", "-=", "*=", "/=", "%=", "&=", "^=", "|=",  // Assignment operators
        ]
    }

    fn functions(&self) -> Vec<&str> {
        vec![
            // String aggregation
            "STRING_AGG",
            // String functions
            "CONCAT_WS", "STRING_SPLIT", "FORMAT", "CHARINDEX", "PATINDEX",
            "LEFT", "RIGHT", "REVERSE", "REPLICATE", "SPACE", "STUFF",
            "QUOTENAME", "SOUNDEX", "DIFFERENCE",
            // Date functions
            "DATEADD", "DATEDIFF", "DATEDIFF_BIG", "DATEFROMPARTS", "TIMEFROMPARTS",
            "DATETIMEFROMPARTS", "EOMONTH", "ISDATE",
            "SYSDATETIME", "SYSUTCDATETIME", "SYSDATETIMEOFFSET",
            "GETDATE", "GETUTCDATE", "CURRENT_TIMESTAMP",
            // Conversion functions
            "CAST", "CONVERT", "TRY_CAST", "TRY_CONVERT", "PARSE", "TRY_PARSE",
            // JSON functions
            "OPENJSON", "JSON_VALUE", "JSON_QUERY", "JSON_MODIFY", "ISJSON",
            "FOR JSON AUTO", "FOR JSON PATH",
            // XML functions
            "FOR XML AUTO", "FOR XML PATH", "FOR XML EXPLICIT", "FOR XML RAW",
            "OPENXML",
            // Window functions
            "ROW_NUMBER", "RANK", "DENSE_RANK", "NTILE",
            "LAG", "LEAD", "FIRST_VALUE", "LAST_VALUE",
            "PERCENT_RANK", "CUME_DIST", "PERCENTILE_CONT", "PERCENTILE_DISC",
            // Aggregate functions
            "CHECKSUM_AGG", "STDEV", "STDEVP", "VAR", "VARP",
            "GROUPING", "GROUPING_ID",
            // System functions
            "@@IDENTITY", "@@ROWCOUNT", "@@ERROR", "@@TRANCOUNT",
            "SCOPE_IDENTITY", "IDENT_CURRENT", "NEWID", "NEWSEQUENTIALID",
            "DB_NAME", "OBJECT_NAME", "SCHEMA_NAME",
            "USER_NAME", "SUSER_NAME", "ORIGINAL_LOGIN",
            // Cryptographic functions
            "HASHBYTES", "ENCRYPTBYKEY", "DECRYPTBYKEY",
            "ENCRYPTBYPASSPHRASE", "DECRYPTBYPASSPHRASE",
        ]
    }

    fn parse_statement(&self, _sql: &str) -> Result<Option<Statement>, Box<dyn Error>> {
        // SQL Server-specific statements not yet implemented
        Ok(None)
    }

    fn documentation_url(&self) -> Option<&str> {
        Some("https://learn.microsoft.com/en-us/sql/t-sql/")
    }

    fn min_version(&self) -> Option<&str> {
        Some("2017")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sqlserver_extension() {
        let ext = SQLServerExtension;
        assert_eq!(ext.name(), "sqlserver");

        // Check TOP clause
        let keywords = ext.keywords();
        assert!(keywords.contains(&"TOP"));
        assert!(keywords.contains(&"OUTPUT"));

        // Check MATCH for graph queries
        assert!(keywords.contains(&"MATCH"));
        assert!(keywords.contains(&"NODE"));
        assert!(keywords.contains(&"EDGE"));
    }

    #[test]
    fn test_sqlserver_functions() {
        let ext = SQLServerExtension;
        let functions = ext.functions();

        assert!(functions.contains(&"STRING_AGG"));
        assert!(functions.contains(&"DATEADD"));
        assert!(functions.contains(&"OPENJSON"));
    }
}
