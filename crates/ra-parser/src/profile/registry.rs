//! Global registry of built-in parser profiles.

use super::{ParserProfile, ProfileLoader};
use std::error::Error;
use std::sync::OnceLock;

/// Global registry of parser profiles.
///
/// Provides access to built-in profiles and allows registering custom profiles.
pub struct ProfileRegistry {
    loader: ProfileLoader,
}

static GLOBAL_REGISTRY: OnceLock<ProfileRegistry> = OnceLock::new();

impl ProfileRegistry {
    /// Get the global profile registry instance.
    pub fn global() -> &'static ProfileRegistry {
        GLOBAL_REGISTRY.get_or_init(|| {
            // Default to profiles directory relative to crate root
            let profile_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/profiles");
            ProfileRegistry {
                loader: ProfileLoader::new(profile_dir),
            }
        })
    }

    /// Load a profile by name.
    ///
    /// First checks built-in profiles, then falls back to loading from disk.
    pub fn load(&self, name: &str) -> Result<ParserProfile, Box<dyn Error>> {
        // Check for built-in profiles first
        match name {
            "universal" => Ok(ParserProfile::universal()),
            _ => self.loader.load(name),
        }
    }

    /// List all available profiles.
    pub fn list(&self) -> Result<Vec<String>, Box<dyn Error>> {
        let mut profiles = vec!["universal".to_string()];
        profiles.extend(self.loader.list_profiles()?);
        Ok(profiles)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_global_registry() {
        let registry = ProfileRegistry::global();
        let result = registry.load("universal");
        assert!(result.is_ok());
    }

    #[test]
    fn test_list_profiles() {
        let registry = ProfileRegistry::global();
        let result = registry.list();
        assert!(result.is_ok());
        let profiles = result.unwrap();
        assert!(profiles.contains(&"universal".to_string()));
    }
}
