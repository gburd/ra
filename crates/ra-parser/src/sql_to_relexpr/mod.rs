//! SQL to RelExpr converter using the Lime LALR(1) parser.
//!
//! Supports SQL constructs:
//! - SELECT with projection list, DISTINCT
//! - FROM single table, INNER/LEFT/RIGHT/FULL/CROSS JOIN, subqueries
//! - WHERE with AND, OR, comparison operators
//! - GROUP BY with aggregates (COUNT, SUM, AVG, MIN, MAX, STDDEV, etc.)
//! - HAVING (post-aggregate filter)
//! - ORDER BY with ASC/DESC and NULLS FIRST/LAST
//! - LIMIT/OFFSET
//! - WITH/CTE (Common Table Expressions)
//! - Window functions (ROW_NUMBER, RANK, LAG, LEAD, etc.)
//! - UNION/INTERSECT/EXCEPT set operations

mod api;
mod error;

pub use api::{sql_to_relexpr, sql_to_relexprs};
pub use error::SqlConversionError;

#[cfg(test)]
#[expect(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;
