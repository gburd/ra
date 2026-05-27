//! `ra-plan-advice` — a clean-room Rust port of `PostgreSQL`'s
//! [`pg_plan_advice`][pgpa] contrib module.
//!
//! [pgpa]: https://www.postgresql.org/docs/current/pgplanadvice.html
//!
//! This crate parses, manipulates, and renders the same plan-advice
//! mini-language that `PostgreSQL` 19 introduced. Strings emitted by
//! this crate are intended to round-trip through PG's parser and vice
//! versa, allowing Ra to participate in the plan-advice ecosystem
//! both as a standalone optimizer and as a PG planner-hook drop-in.
//!
//! # Quick start
//!
//! ```rust
//! use ra_plan_advice::{parse_advice, render_advice};
//!
//! let s = "JOIN_ORDER(f d) HASH_JOIN(d) SEQ_SCAN(f d)";
//! let advice = parse_advice(s).expect("valid advice");
//! assert_eq!(advice.len(), 3);
//! let rendered = render_advice(&advice);
//! // The rendered string parses back to the same AST:
//! let reparsed = parse_advice(&rendered).unwrap();
//! assert_eq!(advice, reparsed);
//! ```
//!
//! # Module layout
//!
//! - [`ast`] — the typed AST mirroring `pgpa_ast.h`.
//! - [`parser`] — recursive-descent parser matching `pgpa_parser.y`
//!   and `pgpa_scanner.l`.
//! - [`render`] — AST → string renderer (the round-trip pair to
//!   the parser).
//! - [`mask`] — `PGS_*` strategy-mask bits, pinned to PG's layout
//!   in `pathnodes.h:66-86` for round-trip portability.
//! - [`feedback`] — `PGPA_FB_*` flags + the exact PG wording
//!   ("matched", "partially matched", etc.) for advice-feedback
//!   rendering.
//! - [`trove`] — lookup structure that maps relation identifiers to
//!   advice items, mirroring `pgpa_trove.c`.
//!
//! # Compatibility commitments
//!
//! 1. Identifier syntax (`alias#n/schema.name@plan`) matches PG.
//! 2. Tag spellings, keyword case-insensitivity, double-quote
//!    handling, C-style comments — all match PG exactly.
//! 3. Strategy-mask bit values match PG's `pathnodes.h` so generated
//!    advice is portable between Ra and PG without reinterpretation.
//! 4. Feedback wording matches PG so log filtering between the two
//!    is interoperable.
//!
//! See also: `docs/research/pg-plan-advice-port.md` (the full
//! implementation plan) and `docs/research/geqo-vs-ra.md`
//! (background on PG planner extensibility).

#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod ast;
pub mod feedback;
pub mod mask;
pub mod parser;
pub mod render;
pub mod trove;

pub use ast::{
    Advice, AdviceItem, AdviceTag, AdviceTarget, AdviceTargetKind,
    IndexTarget, RelationIdentifier,
};
pub use feedback::{FeedbackFlags, format_feedback};
pub use mask::PgsMask;
pub use parser::{parse_advice, ParseError};
pub use render::render_advice;
pub use trove::{Trove, TroveLookup};
