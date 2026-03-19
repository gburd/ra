//! Parser for .rra (Relational Rule Algebra) literate format.
//!
//! This crate parses rule files written in literate markdown format,
//! extracting frontmatter, documentation, and code blocks.

#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]

pub mod extractor;
pub mod formatter;
pub mod lexer;
pub mod match_recognize;
pub mod parser;
pub mod rule_registry;
pub mod sql_to_relexpr;
pub mod test_case;
pub mod validator;

pub use extractor::*;
pub use formatter::*;
pub use match_recognize::*;
pub use parser::*;
pub use rule_registry::*;
pub use sql_to_relexpr::*;
pub use test_case::*;
pub use validator::*;
