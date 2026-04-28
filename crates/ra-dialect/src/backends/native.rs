//! Native SQL dialect translation backend.
//!
//! This module parses SQL into `RelExpr` via `ra-parser` and then
//! emits dialect-specific SQL via the emitter.

use ra_parser::sql_to_relexprs;

use crate::dialect::Dialect;
use crate::emitter::emit_sql;
use crate::error::TranslationError;
use crate::{Backend, TranslationResult};

/// Native translation backend implementation.
pub struct NativeBackend;

impl Backend for NativeBackend {
    fn translate(
        &self,
        sql: &str,
        _source: Dialect,
        target: Dialect,
    ) -> Result<TranslationResult, TranslationError> {
        let rel_exprs = sql_to_relexprs(sql)
            .map_err(|e| TranslationError::Parse(e.to_string()))?;

        let mut all_warnings = Vec::new();
        let mut translated_stmts = Vec::new();

        for rel_expr in &rel_exprs {
            let result = emit_sql(rel_expr, target)?;
            all_warnings.extend(result.warnings);
            translated_stmts.push(result.sql);
        }

        let sql = translated_stmts.join(";\n");

        Ok(TranslationResult {
            sql,
            warnings: all_warnings,
        })
    }
}
