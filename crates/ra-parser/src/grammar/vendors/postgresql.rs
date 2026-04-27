//! PostgreSQL-specific SQL grammar extensions.
//!
//! PostgreSQL extends standard SQL with powerful features including arrays, JSONB,
//! and many custom operators.
//!
//! # Key Features
//!
//! ## Array Types
//!
//! ```sql
//! SELECT ARRAY[1,2,3]::int[];
//! SELECT '{1,2,3}'::int[];
//! SELECT array_length(my_array, 1);
//! ```
//!
//! ## JSONB Operators
//!
//! ```sql
//! SELECT data @> '{"name": "John"}'::jsonb;  -- Contains
//! SELECT data @? '$.tags[*] ? (@ == "active")';  -- Path exists
//! SELECT data -> 'user' ->> 'name';  -- Extract text
//! ```
//!
//! ## Type Casting
//!
//! PostgreSQL uses `::` for type casting:
//! ```sql
//! SELECT '123'::integer;
//! SELECT now()::date;
//! ```
//!
//! ## Dollar Quoting
//!
//! ```sql
//! SELECT $$This is a string$$;
//! SELECT $tag$String with $$ inside$tag$;
//! ```
//!
//! ## RETURNING Clause
//!
//! ```sql
//! INSERT INTO users (name) VALUES ('Alice') RETURNING id, created_at;
//! UPDATE users SET active = true WHERE id = 1 RETURNING *;
//! DELETE FROM users WHERE id = 1 RETURNING *;
//! ```

use sqlparser::ast::Statement;
use std::error::Error;

use crate::grammar::extension::GrammarExtension;

/// PostgreSQL-specific extension.
pub struct PostgreSQLExtension;

impl GrammarExtension for PostgreSQLExtension {
    fn name(&self) -> &str {
        "postgresql"
    }

    fn keywords(&self) -> Vec<&str> {
        vec![
            // RETURNING clause
            "RETURNING",
            // UPSERT (INSERT...ON CONFLICT)
            "ON CONFLICT",
            "DO NOTHING",
            "DO UPDATE",
            // Array operations
            "ARRAY",
            // Window frame exclusion
            "EXCLUDE",
            "CURRENT ROW",
            "GROUP",
            "TIES",
            // LATERAL joins
            "LATERAL",
            // TABLESAMPLE
            "TABLESAMPLE",
            "BERNOULLI",
            "SYSTEM",
            // String constants
            "E", // Escape strings: E'foo\nbar'
            // Type modifiers
            "COLLATE",
            // DISTINCT ON
            "DISTINCT ON",
            // SELECT INTO
            "INTO",
            // VACUUM, ANALYZE
            "VACUUM",
            "ANALYZE",
            // LISTEN/NOTIFY
            "LISTEN",
            "NOTIFY",
            "UNLISTEN",
            // COPY
            "COPY",
            "STDIN",
            "STDOUT",
        ]
    }

    fn operators(&self) -> Vec<&str> {
        vec![
            // Type casting
            "::", // JSONB operators
            "@>",  // Contains (JSONB, array, range)
            "<@",  // Contained by
            "@?",  // Path exists (JSON path query)
            "@@",  // JSON path predicate
            "?",   // Key exists
            "?|",  // Any key exists
            "?&",  // All keys exist
            "#>",  // Get JSON at path (returns JSON)
            "#>>", // Get JSON at path (returns text)
            "->",  // Get JSON field (returns JSON)
            "->>", // Get JSON field (returns text)
            "#-",  // Delete path
            // Array operators
            "||", // Array concatenation (also string concat)
            "&&", // Array overlap
            // Range operators
            "-|-", // Adjacent to
            "<<",  // Strictly left of
            ">>",  // Strictly right of
            "&<",  // Does not extend right of
            "&>",  // Does not extend left of
            // Pattern matching
            "~",    // Matches regex (case sensitive)
            "~*",   // Matches regex (case insensitive)
            "!~",   // Does not match regex (case sensitive)
            "!~*",  // Does not match regex (case insensitive)
            "~~",   // LIKE
            "~~*",  // ILIKE
            "!~~",  // NOT LIKE
            "!~~*", // NOT ILIKE
            // Text search
            "@@@", // Text search match
            // Geometric operators (subset)
            "@-@", // Length
            "@@",  // Center point
            "<->", // Distance
            // Network operators
            "<<",  // Is subnet of
            "<<=", // Is subnet of or equals
            ">>",  // Contains subnet
            ">>=", // Contains subnet or equals
            "&&",  // Overlaps (network)
        ]
    }

    fn functions(&self) -> Vec<&str> {
        vec![
            // Array functions
            "array_length",
            "array_position",
            "array_positions",
            "array_append",
            "array_prepend",
            "array_cat",
            "array_agg",
            "unnest",
            // JSONB functions
            "jsonb_build_object",
            "jsonb_build_array",
            "jsonb_object",
            "jsonb_agg",
            "jsonb_object_agg",
            "jsonb_set",
            "jsonb_insert",
            "jsonb_strip_nulls",
            "jsonb_pretty",
            "jsonb_typeof",
            "jsonb_path_exists",
            "jsonb_path_query",
            // String functions
            "concat_ws",
            "format",
            "regexp_replace",
            "regexp_split_to_array",
            "regexp_split_to_table",
            "regexp_match",
            "regexp_matches",
            "string_agg",
            "string_to_array",
            "array_to_string",
            // Date/time functions
            "date_trunc",
            "date_part",
            "age",
            "justify_days",
            "justify_hours",
            "justify_interval",
            "generate_series",
            "to_timestamp",
            "to_char",
            // Range functions
            "numrange",
            "int4range",
            "int8range",
            "daterange",
            "tsrange",
            "tstzrange",
            "lower",
            "upper",
            "isempty",
            "lower_inc",
            "upper_inc",
            "lower_inf",
            "upper_inf",
            // Window functions (PostgreSQL-specific)
            "cume_dist",
            "dense_rank",
            "ntile",
            "percent_rank",
            "rank",
            "row_number",
            // Aggregate functions
            "bool_and",
            "bool_or",
            "every",
            "mode",
            "percentile_cont",
            "percentile_disc",
            // System functions
            "pg_sleep",
            "pg_advisory_lock",
            "pg_advisory_unlock",
            "pg_column_size",
            "pg_database_size",
            "pg_table_size",
            "current_database",
            "current_schema",
            "current_schemas",
            "version",
        ]
    }

    fn parse_statement(&self, _sql: &str) -> Result<Option<Statement>, Box<dyn Error>> {
        // PostgreSQL-specific statements not yet implemented
        Ok(None)
    }

    fn documentation_url(&self) -> Option<&str> {
        Some("https://www.postgresql.org/docs/current/")
    }

    fn min_version(&self) -> Option<&str> {
        Some("9.6")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_postgresql_extension() {
        let ext = PostgreSQLExtension;
        assert_eq!(ext.name(), "postgresql");

        // Check RETURNING clause
        let keywords = ext.keywords();
        assert!(keywords.contains(&"RETURNING"));
        assert!(keywords.contains(&"ON CONFLICT"));

        // Check :: operator
        let operators = ext.operators();
        assert!(operators.contains(&"::"));

        // Check JSONB operators
        assert!(operators.contains(&"@>"));
        assert!(operators.contains(&"->"));
        assert!(operators.contains(&"->>"));
    }

    #[test]
    fn test_array_functions() {
        let ext = PostgreSQLExtension;
        let functions = ext.functions();

        assert!(functions.contains(&"array_length"));
        assert!(functions.contains(&"array_agg"));
        assert!(functions.contains(&"unnest"));
    }

    #[test]
    fn test_jsonb_functions() {
        let ext = PostgreSQLExtension;
        let functions = ext.functions();

        assert!(functions.contains(&"jsonb_build_object"));
        assert!(functions.contains(&"jsonb_agg"));
        assert!(functions.contains(&"jsonb_set"));
    }
}
