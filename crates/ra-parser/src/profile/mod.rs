//! Parser profile system for SQL dialect and version support.
//!
//! Profiles define which SQL features, extensions, and syntax variants are
//! supported for a given database vendor and version.

pub mod loader;
pub mod registry;

pub use loader::ProfileLoader;
pub use registry::ProfileRegistry;

use std::collections::HashMap;
use std::error::Error;

/// Validation configuration for parser strictness.
#[derive(Debug, Clone, Default)]
pub struct ValidationConfig {
    /// Enable strict type checking during parsing
    pub strict_type_checking: bool,
    /// Enforce strict function arity checking
    pub strict_function_arity: bool,
    /// Warn on ambiguous SQL syntax
    pub warn_on_ambiguous_syntax: bool,
}

/// A parser profile defining supported SQL features.
///
/// Profiles can be composed from multiple sources (standards, vendors, extensions)
/// and support version inheritance.
#[derive(Debug, Clone)]
pub struct ParserProfile {
    name: String,
    /// Database vendor (e.g., "postgresql", "mysql")
    pub vendor: Option<String>,
    /// Vendor version (e.g., "17", "8.4")
    pub version: Option<String>,
    /// Parent profile name for inheritance
    pub inherits_from: Option<String>,
    /// Feature flags from TOML (e.g., `sql_92 = true`, `sql_2023 = true`)
    pub features: HashMap<String, bool>,
    /// Custom syntax options (e.g., `backticks = "true"` for `MySQL`)
    pub syntax: HashMap<String, String>,
    /// Supported operators (e.g., "@>", "@=", "<->" from vendor/extension profiles)
    pub operators: Vec<String>,
    /// Supported function names (e.g., `string_agg`, `GROUP_CONCAT`)
    pub functions: Vec<String>,
    /// Validation rules for parsing strictness
    pub validation: ValidationConfig,
}

impl ParserProfile {
    /// Get the profile name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Create the universal profile (supports all dialects).
    #[must_use]
    pub fn universal() -> Self {
        Self {
            name: "universal".to_string(),
            vendor: None,
            version: None,
            inherits_from: None,
            features: HashMap::new(),
            syntax: HashMap::new(),
            operators: Vec::new(),
            functions: Vec::new(),
            validation: ValidationConfig::default(),
        }
    }

    /// Load a profile by name from the profile registry.
    ///
    /// Supports composition syntax using `+` to combine profiles:
    /// - "postgresql-17" - Single profile
    /// - "postgresql-17+postgis" - Base + extension
    /// - "postgresql-17+postgis+timescaledb" - Base + multiple extensions
    ///
    /// # Arguments
    ///
    /// * `name` - Profile name (e.g., "postgresql-17", "mysql-8.4")
    ///
    /// # Errors
    ///
    /// Returns an error if the profile is not found.
    pub fn load(name: &str) -> Result<Self, Box<dyn Error>> {
        // Check for composition syntax
        if name.contains('+') {
            Self::load_composed(name)
        } else {
            ProfileRegistry::global().load(name)
        }
    }

    /// Load a composed profile from base + extensions.
    ///
    /// # Arguments
    ///
    /// * `name` - Composed profile name (e.g., "postgresql-17+postgis+timescaledb")
    fn load_composed(name: &str) -> Result<Self, Box<dyn Error>> {
        let parts: Vec<&str> = name.split('+').collect();

        if parts.is_empty() {
            return Err("Empty profile name".into());
        }

        // Load base profile (first part)
        let mut profile = ProfileRegistry::global().load(parts[0])?;

        // Load and merge extension profiles
        for extension_name in &parts[1..] {
            let extension = ProfileRegistry::global().load_extension(extension_name)?;

            // Merge extension features, operators, functions into base profile
            profile.features.extend(extension.features);
            profile.syntax.extend(extension.syntax);

            // Add operators from extension (avoiding duplicates)
            for op in extension.operators {
                if !profile.operators.contains(&op) {
                    profile.operators.push(op);
                }
            }

            // Add functions from extension (avoiding duplicates)
            for func in extension.functions {
                if !profile.functions.contains(&func) {
                    profile.functions.push(func);
                }
            }
        }

        // Update profile name to reflect composition
        profile.name = name.to_string();

        Ok(profile)
    }

    /// Infer the best profile from SQL text.
    ///
    /// Returns the detected profile and a confidence score (0.0-1.0).
    ///
    /// # Errors
    ///
    /// Returns an error if dialect detection fails or the inferred profile
    /// cannot be loaded.
    pub fn infer(sql: &str) -> Result<(Self, f64), Box<dyn Error>> {
        use crate::parser::inference::DialectInference;

        let mut inference = DialectInference::new();
        inference.detect_from_tokens(sql);
        inference.detect_from_syntax(sql);
        inference.detect_from_functions(sql);

        let (dialect, confidence) = inference.compute_scores();

        // Map dialect to profile name (use latest version by default)
        let profile_name = match dialect.as_str() {
            "postgresql" => "postgresql-17",
            "mysql" => "mysql-8.4",
            "oracle" => "oracle-21c",
            "sqlserver" => "sqlserver-2022",
            _ => "universal",
        };

        let profile = Self::load(profile_name)?;
        Ok((profile, confidence))
    }
}

#[expect(clippy::unwrap_used, reason = "test code")]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_universal_profile() {
        let profile = ParserProfile::universal();
        assert_eq!(profile.name(), "universal");
    }

    #[test]
    fn test_load_vendor_profile() {
        let result = ParserProfile::load("postgresql-17");
        assert!(result.is_ok(), "Failed to load postgresql-17 profile");

        let profile = result.unwrap();
        assert_eq!(profile.name(), "postgresql-17");
    }

    #[test]
    fn test_load_oracle_profile() {
        let result = ParserProfile::load("oracle-21c");
        assert!(result.is_ok(), "Failed to load oracle-21c profile");

        let profile = result.unwrap();
        assert_eq!(profile.name(), "oracle-21c");
    }

    #[test]
    fn test_profile_composition() {
        let result = ParserProfile::load("postgresql-17+postgis");
        assert!(result.is_ok(), "Failed to load composed profile");

        let profile = result.unwrap();
        assert_eq!(profile.name(), "postgresql-17+postgis");
    }

    #[test]
    fn test_multi_extension_composition() {
        let result = ParserProfile::load("postgresql-17+postgis+timescaledb");
        assert!(result.is_ok(), "Failed to load multi-extension profile");

        let profile = result.unwrap();
        assert_eq!(profile.name(), "postgresql-17+postgis+timescaledb");
    }

    #[test]
    fn test_dialect_inference_postgresql() {
        let sql = "SELECT ARRAY[1,2,3]::int[] FROM users WHERE data @> '{\"key\": \"value\"}'";
        let result = ParserProfile::infer(sql);

        assert!(result.is_ok(), "Dialect inference failed");
        let (profile, confidence) = result.unwrap();

        // Should detect PostgreSQL
        assert!(profile.name().contains("postgresql"));
        assert!(confidence > 0.5, "Confidence too low: {confidence}");
    }

    #[test]
    fn test_dialect_inference_mysql() {
        let sql = "SELECT * FROM `users` WHERE id = 1 LIMIT 10, 5";
        let result = ParserProfile::infer(sql);

        assert!(result.is_ok(), "Dialect inference failed");
        let (profile, confidence) = result.unwrap();

        // Should detect MySQL
        assert!(profile.name().contains("mysql"));
        assert!(confidence > 0.5, "Confidence too low: {confidence}");
    }
}
