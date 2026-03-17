//! Compatibility matrix for cross-dialect feature support.
//!
//! Provides a structured overview of which SQL features are
//! supported natively, emulated, or unsupported across all
//! dialects.

use crate::dialect::{feature_support, Dialect, FeatureSupport, SqlFeature};
use std::collections::BTreeMap;
use std::fmt::Write;

/// A compatibility matrix showing feature support across
/// dialects.
#[derive(Debug)]
pub struct CompatibilityMatrix {
    entries: BTreeMap<String, Vec<(Dialect, FeatureSupport)>>,
}

impl CompatibilityMatrix {
    /// Build the full compatibility matrix for all dialects
    /// and features.
    #[must_use]
    pub fn build() -> Self {
        let features = [
            SqlFeature::Limit,
            SqlFeature::Offset,
            SqlFeature::Fetch,
            SqlFeature::BooleanLiterals,
            SqlFeature::ConcatOperator,
            SqlFeature::Ilike,
            SqlFeature::FullOuterJoin,
            SqlFeature::Except,
            SqlFeature::Coalesce,
            SqlFeature::Nullif,
            SqlFeature::Cast,
            SqlFeature::Length,
            SqlFeature::Substring,
            SqlFeature::CurrentTimestamp,
            SqlFeature::DateExtract,
        ];

        let mut entries = BTreeMap::new();

        for feature in &features {
            let mut support = Vec::new();
            for dialect in &Dialect::ALL {
                support.push((*dialect, feature_support(*dialect, *feature)));
            }
            entries.insert(feature.to_string(), support);
        }

        Self { entries }
    }

    /// Get the support level for a specific feature and
    /// dialect.
    #[must_use]
    pub fn get(&self, feature: &str, dialect: Dialect) -> Option<FeatureSupport> {
        self.entries
            .get(feature)
            .and_then(|entries| entries.iter().find(|(d, _)| *d == dialect).map(|(_, s)| *s))
    }

    /// Return all features in the matrix.
    #[must_use]
    pub fn features(&self) -> Vec<&str> {
        self.entries.keys().map(String::as_str).collect()
    }

    /// Format the matrix as a human-readable table.
    #[must_use]
    pub fn to_table(&self) -> String {
        let mut lines = Vec::new();

        // Header
        let mut header = format!("{:<20}", "Feature");
        for dialect in &Dialect::ALL {
            let _ = write!(header, "{dialect:<12}");
        }
        lines.push(header);

        // Separator
        lines.push("-".repeat(20 + 12 * Dialect::ALL.len()));

        // Rows
        for (feature, support) in &self.entries {
            let mut row = format!("{feature:<20}");
            for (_, level) in support {
                let symbol = match level {
                    FeatureSupport::Native => "native",
                    FeatureSupport::Emulated => "emulated",
                    FeatureSupport::Unsupported => "unsupported",
                };
                let _ = write!(row, "{symbol:<12}");
            }
            lines.push(row);
        }

        lines.join("\n")
    }

    /// Count features with native support for a given
    /// dialect.
    #[must_use]
    pub fn native_count(&self, dialect: Dialect) -> usize {
        self.count_support(dialect, FeatureSupport::Native)
    }

    /// Count features that require emulation for a given
    /// dialect.
    #[must_use]
    pub fn emulated_count(&self, dialect: Dialect) -> usize {
        self.count_support(dialect, FeatureSupport::Emulated)
    }

    /// Count unsupported features for a given dialect.
    #[must_use]
    pub fn unsupported_count(&self, dialect: Dialect) -> usize {
        self.count_support(dialect, FeatureSupport::Unsupported)
    }

    fn count_support(&self, dialect: Dialect, target: FeatureSupport) -> usize {
        self.entries
            .values()
            .filter(|support| support.iter().any(|(d, s)| *d == dialect && *s == target))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_matrix() {
        let matrix = CompatibilityMatrix::build();
        assert!(!matrix.features().is_empty());
    }

    #[test]
    fn matrix_get() {
        let matrix = CompatibilityMatrix::build();
        assert_eq!(
            matrix.get("LIMIT", Dialect::PostgreSql),
            Some(FeatureSupport::Native)
        );
        assert_eq!(
            matrix.get("LIMIT", Dialect::MsSql),
            Some(FeatureSupport::Emulated)
        );
    }

    #[test]
    fn matrix_get_nonexistent() {
        let matrix = CompatibilityMatrix::build();
        assert_eq!(matrix.get("NONEXISTENT", Dialect::PostgreSql), None);
    }

    #[test]
    fn matrix_table_format() {
        let matrix = CompatibilityMatrix::build();
        let table = matrix.to_table();
        assert!(table.contains("Feature"));
        assert!(table.contains("PostgreSQL"));
        assert!(table.contains("native"));
    }

    #[test]
    fn postgres_has_most_native_support() {
        let matrix = CompatibilityMatrix::build();
        let pg_native = matrix.native_count(Dialect::PostgreSql);
        let sqlite_native = matrix.native_count(Dialect::Sqlite);
        assert!(
            pg_native >= sqlite_native,
            "PostgreSQL ({pg_native}) should have >= \
             SQLite ({sqlite_native}) native support"
        );
    }

    #[test]
    fn unsupported_features_exist() {
        let matrix = CompatibilityMatrix::build();
        let mysql_unsupported = matrix.unsupported_count(Dialect::MySql);
        assert!(
            mysql_unsupported > 0,
            "MySQL should have some unsupported features"
        );
    }

    #[test]
    fn counts_add_up() {
        let matrix = CompatibilityMatrix::build();
        let total = matrix.features().len();
        for dialect in &Dialect::ALL {
            let n = matrix.native_count(*dialect);
            let e = matrix.emulated_count(*dialect);
            let u = matrix.unsupported_count(*dialect);
            assert_eq!(
                n + e + u,
                total,
                "Counts for {dialect} don't add up: \
                 {n} + {e} + {u} != {total}"
            );
        }
    }
}
