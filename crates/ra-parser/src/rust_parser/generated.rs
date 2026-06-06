//! The Lime-generated Rust LALR parser, included verbatim from `OUT_DIR`.
//!
//! `build.rs` runs `lime --target=rust` on `grammar/ra_sql.lime` to produce
//! `ra_sql.rs`. The reduction actions in that file call the native builders by
//! bare name (`ra_scan`, `ra_cte`, …) and use the `Value` type alias the
//! grammar declares via `%rust_value_type`, so both are brought into scope
//! here before the include.
//!
//! Generated code is exempt from the workspace lint profile.
#![allow(
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    clippy::restriction,
    dead_code,
    non_snake_case,
    non_camel_case_types,
    unused
)]

// Builders invoked by bare name inside the generated reduction actions.
use crate::rust_parser::builders::*;

include!(concat!(env!("OUT_DIR"), "/ra_sql.rs"));
