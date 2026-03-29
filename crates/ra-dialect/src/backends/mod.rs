//! Backend implementations for SQL dialect translation.

pub mod native;

#[cfg(feature = "polyglot-backend")]
pub mod polyglot_backend;

use crate::{Dialect, TranslationError, TranslationResult};

/// Translation backend implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TranslationBackend {
    /// Native translation implementation.
    #[default]
    Native,
    /// Polyglot SQL transpiler backend.
    #[cfg(feature = "polyglot-backend")]
    Polyglot,
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
    ///
    /// # Errors
    ///
    /// Returns error if translation fails due to parsing or unsupported constructs
    fn translate(
        &self,
        sql: &str,
        source: Dialect,
        target: Dialect,
    ) -> Result<TranslationResult, TranslationError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translation_backend_default() {
        let backend = TranslationBackend::default();
        assert_eq!(backend, TranslationBackend::Native);
    }

    #[test]
    fn translation_backend_name() {
        assert_eq!(TranslationBackend::Native.name(), "native");

        #[cfg(feature = "polyglot-backend")]
        assert_eq!(TranslationBackend::Polyglot.name(), "polyglot");
    }

    #[test]
    fn native_backend_supports_standard_dialects() {
        let backend = TranslationBackend::Native;

        assert!(backend.supports_dialect_pair(Dialect::PostgreSql, Dialect::MySql));
        assert!(backend.supports_dialect_pair(Dialect::MySql, Dialect::Sqlite));
        assert!(backend.supports_dialect_pair(Dialect::Sqlite, Dialect::DuckDb));
        assert!(backend.supports_dialect_pair(Dialect::DuckDb, Dialect::MsSql));
        assert!(backend.supports_dialect_pair(Dialect::MsSql, Dialect::Oracle));
        assert!(backend.supports_dialect_pair(Dialect::Oracle, Dialect::PostgreSql));
    }

    #[test]
    fn backend_equality() {
        assert_eq!(TranslationBackend::Native, TranslationBackend::Native);

        #[cfg(feature = "polyglot-backend")]
        {
            assert_eq!(TranslationBackend::Polyglot, TranslationBackend::Polyglot);
            assert_ne!(TranslationBackend::Native, TranslationBackend::Polyglot);
        }
    }

    #[test]
    fn backend_debug_format() {
        let backend = TranslationBackend::Native;
        let debug_str = format!("{backend:?}");
        assert!(debug_str.contains("Native"));
    }

    #[test]
    fn backend_clone() {
        let backend = TranslationBackend::Native;
        let cloned = backend;
        assert_eq!(backend, cloned);
    }

    #[cfg(feature = "polyglot-backend")]
    #[test]
    fn polyglot_backend_supports_all_dialects() {
        let backend = TranslationBackend::Polyglot;

        // Standard dialects
        assert!(backend.supports_dialect_pair(Dialect::PostgreSql, Dialect::MySql));

        // Polyglot backend should support all dialect pairs
        assert!(backend.supports_dialect_pair(Dialect::DuckDb, Dialect::DuckDb));
    }
}