//! Parser profile system for SQL dialect and version support.
//!
//! Profiles define which SQL features, extensions, and syntax variants are
//! supported for a given database vendor and version.

pub mod loader;
pub mod registry;

pub use loader::ProfileLoader;
pub use registry::ProfileRegistry;

use std::error::Error;
use std::fmt;

/// A parser profile defining supported SQL features.
///
/// Profiles can be composed from multiple sources (standards, vendors, extensions)
/// and support version inheritance.
#[derive(Debug, Clone)]
pub struct ParserProfile {
    name: String,
    /// Database vendor (e.g., "postgresql", "mysql"). Used for profile inheritance.
    #[allow(dead_code)]
    vendor: Option<String>,
    /// Vendor version (e.g., "17", "8.4"). Used for version-specific features.
    #[allow(dead_code)]
    version: Option<String>,
    /// Parent profile name for inheritance. Used when loading TOML profiles.
    #[allow(dead_code)]
    inherits_from: Option<String>,
    // TODO: Add grammar extensions, keywords, operators, functions
}

impl ParserProfile {
    /// Get the profile name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Create the universal profile (supports all dialects).
    pub fn universal() -> Self {
        Self {
            name: "universal".to_string(),
            vendor: None,
            version: None,
            inherits_from: None,
        }
    }

    /// Load a profile by name from the profile registry.
    ///
    /// # Arguments
    ///
    /// * `name` - Profile name (e.g., "postgresql-17", "mysql-8.4")
    ///
    /// # Errors
    ///
    /// Returns an error if the profile is not found.
    pub fn load(name: &str) -> Result<Self, Box<dyn Error>> {
        ProfileRegistry::global().load(name)
    }

    /// Infer the best profile from SQL text.
    ///
    /// Returns the detected profile and a confidence score (0.0-1.0).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_universal_profile() {
        let profile = ParserProfile::universal();
        assert_eq!(profile.name(), "universal");
    }
}
