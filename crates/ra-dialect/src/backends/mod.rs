//! Backend implementations for SQL dialect translation.

pub mod native;

#[cfg(feature = "polyglot-backend")]
pub mod polyglot_backend;

use crate::{Dialect, TranslationError, TranslationResult};

/// Translation backend implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranslationBackend {
    /// Native translation implementation.
    Native,
    /// Polyglot SQL transpiler backend.
    #[cfg(feature = "polyglot-backend")]
    Polyglot,
}

impl Default for TranslationBackend {
    fn default() -> Self {
        Self::Native
    }
}

impl TranslationBackend {
    /// Get the name of this backend.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Native => "native",
            #[cfg(feature = "polyglot-backend")]
            Self::Polyglot => "polyglot",
        }
    }

    /// Check if this backend supports the given dialect pair.
    #[must_use]
    pub fn supports_dialect_pair(self, source: Dialect, target: Dialect) -> bool {
        match self {
            Self::Native => {
                // Native backend supports the original 6 dialects
                matches!(
                    source,
                    Dialect::PostgreSql
                        | Dialect::MySql
                        | Dialect::Sqlite
                        | Dialect::DuckDb
                        | Dialect::MsSql
                        | Dialect::Oracle
                ) && matches!(
                    target,
                    Dialect::PostgreSql
                        | Dialect::MySql
                        | Dialect::Sqlite
                        | Dialect::DuckDb
                        | Dialect::MsSql
                        | Dialect::Oracle
                )
            }
            #[cfg(feature = "polyglot-backend")]
            Self::Polyglot => {
                // Polyglot supports all dialects including extended ones
                true
            }
        }
    }
}

/// Trait for backend-specific translation.
pub trait Backend {
    /// Translate SQL from source to target dialect.
    fn translate(
        &self,
        sql: &str,
        source: Dialect,
        target: Dialect,
    ) -> Result<TranslationResult, TranslationError>;
}