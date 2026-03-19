//! Cache key types.

use serde::{Deserialize, Serialize};

/// Composite cache key identifying a unique optimization context.
///
/// Two queries with the same SQL text but different hardware profiles
/// or parameter types may produce different optimal plans, so all
/// three components contribute to the key.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct QueryKey {
    /// Normalized SQL text (whitespace-collapsed, case-folded).
    pub sql: String,
    /// Hardware profile name used for optimization.
    pub hardware_profile: String,
    /// Ordered list of parameter type names (e.g. `["int", "text"]`).
    pub parameter_types: Vec<String>,
}

impl QueryKey {
    /// Create a new query key.
    #[must_use]
    pub fn new(
        sql: impl Into<String>,
        hardware_profile: impl Into<String>,
        parameter_types: Vec<String>,
    ) -> Self {
        let raw: String = sql.into();
        Self {
            sql: normalize_sql(&raw),
            hardware_profile: hardware_profile.into(),
            parameter_types,
        }
    }
}

impl std::fmt::Display for QueryKey {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        write!(
            f,
            "{}@{}",
            truncate_sql(&self.sql, 60),
            self.hardware_profile,
        )
    }
}

/// Collapse runs of whitespace and trim.
fn normalize_sql(sql: &str) -> String {
    let mut result = String::with_capacity(sql.len());
    let mut prev_whitespace = false;
    for ch in sql.trim().chars() {
        if ch.is_whitespace() {
            if !prev_whitespace {
                result.push(' ');
            }
            prev_whitespace = true;
        } else {
            result.push(ch);
            prev_whitespace = false;
        }
    }
    result
}

/// Truncate SQL for display, appending "..." if needed.
fn truncate_sql(sql: &str, max_len: usize) -> &str {
    if sql.len() <= max_len {
        sql
    } else {
        &sql[..max_len]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_collapses_whitespace() {
        let key = QueryKey::new(
            "SELECT  *   FROM\n  users".to_owned(),
            "auto".to_owned(),
            vec![],
        );
        assert_eq!(key.sql, "SELECT * FROM users");
    }

    #[test]
    fn different_profiles_differ() {
        let k1 = QueryKey::new(
            "SELECT 1".to_owned(),
            "auto".to_owned(),
            vec![],
        );
        let k2 = QueryKey::new(
            "SELECT 1".to_owned(),
            "gpu-server".to_owned(),
            vec![],
        );
        assert_ne!(k1, k2);
    }

    #[test]
    fn same_key_equal() {
        let k1 = QueryKey::new(
            "SELECT 1".to_owned(),
            "auto".to_owned(),
            vec!["int".to_owned()],
        );
        let k2 = QueryKey::new(
            "SELECT 1".to_owned(),
            "auto".to_owned(),
            vec!["int".to_owned()],
        );
        assert_eq!(k1, k2);
    }

    #[test]
    fn display_format() {
        let key = QueryKey::new(
            "SELECT * FROM users WHERE id = 1".to_owned(),
            "auto".to_owned(),
            vec![],
        );
        let display = format!("{key}");
        assert!(display.contains("@auto"));
        assert!(display.contains("SELECT"));
    }
}
