//! Executors for table-valued functions and array operations.
//!
//! Provides runtime execution of UNNEST, table-valued functions
//! (e.g., `generate_series`), and lateral joins. These executors
//! implement the [`ExprEvaluator`](crate::recursive::ExprEvaluator)
//! pattern, producing [`Row`](crate::recursive::Row) vectors from
//! relational algebra operators.

pub mod lateral_join;
pub mod table_function;
pub mod unnest;

pub use lateral_join::LateralJoinExecutor;
pub use table_function::TableFunctionExecutor;
pub use unnest::UnnestExecutor;
