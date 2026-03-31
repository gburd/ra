//! Profile loader for TOML-based profile definitions.

use super::ParserProfile;
use serde::Deserialize;
use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::path::Path;

/// TOML structure for profile files.
#[derive(Debug, Deserialize)]
struct ProfileToml {
    profile: ProfileMetadata,
    #[serde(default)]
    features: HashMap<String, bool>,
    #[serde(default)]
    syntax: HashMap<String, toml::Value>,
    #[serde(default)]
    operators: HashMap<String, Vec<String>>,
    #[serde(default)]
    functions: HashMap<String, Vec<String>>,
    #[serde(default)]
    validation: ValidationConfig,
}

/// Profile metadata from [profile] section.
#[derive(Debug, Deserialize)]
struct ProfileMetadata {
    name: String,
    #[serde(default)]
    vendor: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    inherits_from: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]  // Metadata field for documentation purposes
    description: Option<String>,
}

/// Validation configuration from [validation] section.
#[derive(Debug, Default, Deserialize)]
struct ValidationConfig {
    #[serde(default)]
    strict_type_checking: bool,
    #[serde(default)]
    strict_function_arity: bool,
    #[serde(default)]
    warn_on_ambiguous_syntax: bool,
}

/// Loads parser profiles from TOML files.
pub struct ProfileLoader {
    profile_dir: String,
}

impl ProfileLoader {
    /// Create a new profile loader.
    ///
    /// # Arguments
    ///
    /// * `profile_dir` - Directory containing `.toml` profile files
    pub fn new(profile_dir: impl Into<String>) -> Self {
        Self {
            profile_dir: profile_dir.into(),
        }
    }

    /// Load a profile from a TOML file.
    ///
    /// # Arguments
    ///
    /// * `name` - Profile name (without `.toml` extension)
    ///
    /// # Returns
    ///
    /// Returns the loaded profile or an error if the file doesn't exist
    /// or cannot be parsed.
    pub fn load(&self, name: &str) -> Result<ParserProfile, Box<dyn Error>> {
        let path = Path::new(&self.profile_dir).join(format!("{}.toml", name));

        if !path.exists() {
            // Try vendor subdirectories
            let vendor_path = Path::new(&self.profile_dir)
                .join("vendors")
                .join(format!("{}.toml", name));

            if vendor_path.exists() {
                return self.load_from_path(&vendor_path);
            }

            return Err(format!("Profile '{}' not found", name).into());
        }

        self.load_from_path(&path)
    }

    /// Load an extension profile from the extensions/ subdirectory.
    ///
    /// # Arguments
    ///
    /// * `name` - Extension name (e.g., "postgis", "timescaledb")
    pub fn load_extension(&self, name: &str) -> Result<ParserProfile, Box<dyn Error>> {
        let extension_path = Path::new(&self.profile_dir)
            .join("extensions")
            .join(format!("{}.toml", name));

        if !extension_path.exists() {
            return Err(format!("Extension profile '{}' not found", name).into());
        }

        self.load_from_path(&extension_path)
    }

    /// Load a profile from a specific file path.
    fn load_from_path(&self, path: &Path) -> Result<ParserProfile, Box<dyn Error>> {
        let contents = fs::read_to_string(path)?;
        let toml: ProfileToml = toml::from_str(&contents)?;

        // Extract operators and functions from TOML
        let operators = toml.operators.values()
            .flat_map(|ops| ops.iter().cloned())
            .collect();

        let functions = toml.functions.values()
            .flat_map(|funcs| funcs.iter().cloned())
            .collect();

        // Convert syntax HashMap<String, toml::Value> to HashMap<String, String>
        let syntax = toml.syntax.into_iter()
            .map(|(k, v)| (k, v.to_string()))
            .collect();

        Ok(ParserProfile {
            name: toml.profile.name,
            vendor: toml.profile.vendor,
            version: toml.profile.version,
            inherits_from: toml.profile.inherits_from,
            features: toml.features,
            syntax,
            operators,
            functions,
            validation: super::ValidationConfig {
                strict_type_checking: toml.validation.strict_type_checking,
                strict_function_arity: toml.validation.strict_function_arity,
                warn_on_ambiguous_syntax: toml.validation.warn_on_ambiguous_syntax,
            },
        })
    }

    /// Load a profile with inheritance resolution.
    ///
    /// If the profile specifies `inherits_from`, recursively load and merge
    /// parent profiles.
    pub fn load_with_inheritance(&self, name: &str) -> Result<ParserProfile, Box<dyn Error>> {
        let mut profile = self.load(name)?;

        // If this profile inherits from another, load the parent
        if let Some(parent_name) = profile.inherits_from.clone() {
            let parent = self.load_with_inheritance(&parent_name)?;

            // Merge parent into current profile
            // For now, just use the current profile's values
            // TODO: Implement actual feature merging
            profile.inherits_from = Some(parent.name);
        }

        Ok(profile)
    }

    /// List all available profiles in the profile directory.
    pub fn list_profiles(&self) -> Result<Vec<String>, Box<dyn Error>> {
        let mut profiles = Vec::new();
        let path = Path::new(&self.profile_dir);

        if !path.exists() {
            return Ok(profiles);
        }

        // List profiles in root directory
        self.collect_profiles(path, &mut profiles)?;

        // List profiles in vendors subdirectory
        let vendors_path = path.join("vendors");
        if vendors_path.exists() {
            self.collect_profiles(&vendors_path, &mut profiles)?;
        }

        profiles.sort();
        profiles.dedup();
        Ok(profiles)
    }

    /// Collect profile names from a directory.
    fn collect_profiles(
        &self,
        dir: &Path,
        profiles: &mut Vec<String>,
    ) -> Result<(), Box<dyn Error>> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if let Some(ext) = path.extension() {
                if ext == "toml" {
                    if let Some(stem) = path.file_stem() {
                        profiles.push(stem.to_string_lossy().to_string());
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loader_creation() {
        let loader = ProfileLoader::new("profiles");
        assert!(!loader.profile_dir.is_empty());
    }

    #[test]
    fn test_parse_universal_profile() {
        let toml_str = r#"
[profile]
name = "universal"
description = "Universal SQL parser"

[features]
sql_92 = true
sql_1999 = true

[syntax]
double_quotes = true
backticks = true

[validation]
strict_type_checking = false
"#;

        let parsed: ProfileToml = toml::from_str(toml_str).expect("should parse");
        assert_eq!(parsed.profile.name, "universal");
        assert!(parsed.features.get("sql_92").copied().unwrap_or(false));
    }
}
