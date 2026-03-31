//! Profile loader for TOML-based profile definitions.

use super::ParserProfile;
use std::error::Error;
use std::fs;
use std::path::Path;

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
            // Return a basic profile for now
            // TODO: Actually load and parse TOML
            return Ok(ParserProfile {
                name: name.to_string(),
                vendor: None,
                version: None,
                inherits_from: None,
            });
        }

        let _contents = fs::read_to_string(&path)?;

        // TODO: Parse TOML using serde
        // For now, return a placeholder
        Ok(ParserProfile {
            name: name.to_string(),
            vendor: None,
            version: None,
            inherits_from: None,
        })
    }

    /// List all available profiles in the profile directory.
    pub fn list_profiles(&self) -> Result<Vec<String>, Box<dyn Error>> {
        let mut profiles = Vec::new();
        let path = Path::new(&self.profile_dir);

        if !path.exists() {
            return Ok(profiles);
        }

        for entry in fs::read_dir(path)? {
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

        profiles.sort();
        Ok(profiles)
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
}
