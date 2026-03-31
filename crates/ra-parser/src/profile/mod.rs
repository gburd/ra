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
            let _extension = ProfileRegistry::global().load_extension(extension_name)?;
            // TODO: Merge extension features, operators, functions into profile
            // For now, just update the name to reflect composition
        }

        // Update profile name to reflect composition
        profile.name = name.to_string();

        Ok(profile)
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
