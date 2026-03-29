//! Layered config file loader.
//!
//! Loads config from multiple sources in priority order:
//! defaults -> /etc/ra/config.toml -> ~/.config/ra/config.toml
//! -> ./.ra/config.toml -> env vars.

use std::path::{Path, PathBuf};

use crate::error::ConfigError;
use crate::model::RaConfig;

/// Returns the user config directory (`~/.config/ra`).
#[must_use]
pub fn config_dir() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".config").join("ra"))
}

/// Returns the user config file path (`~/.config/ra/config.toml`).
#[must_use]
pub fn config_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("config.toml"))
}

/// Layered configuration loader.
///
/// Loads and merges config from multiple files plus environment
/// variables.
pub struct ConfigLoader {
    /// System-wide config path.
    system: PathBuf,
    /// User config path.
    user: Option<PathBuf>,
    /// Local (project) config path.
    local: PathBuf,
}

impl ConfigLoader {
    /// Create a loader with standard paths.
    #[must_use]
    pub fn new() -> Self {
        Self {
            system: PathBuf::from("/etc/ra/config.toml"),
            user: config_path(),
            local: PathBuf::from(".ra/config.toml"),
        }
    }

    /// Override the local config path (for testing).
    #[must_use]
    pub fn with_local(mut self, path: PathBuf) -> Self {
        self.local = path;
        self
    }

    /// Override the user config path (for testing).
    #[must_use]
    pub fn with_user(mut self, path: PathBuf) -> Self {
        self.user = Some(path);
        self
    }

    /// Override the system config path (for testing).
    #[must_use]
    pub fn with_system(mut self, path: PathBuf) -> Self {
        self.system = path;
        self
    }

    /// Load config from all layers.
    ///
    /// Missing files are silently skipped (only defaults apply).
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::ReadFile`] if a config file exists but
    /// cannot be read, or [`ConfigError::ParseToml`] if the TOML
    /// content is invalid.
    pub fn load(&self) -> Result<RaConfig, ConfigError> {
        let mut config = RaConfig::default();

        if let Some(layer) =
            load_optional_file(&self.system)?
        {
            config.merge(&layer);
        }

        if let Some(ref user) = self.user {
            if let Some(layer) =
                load_optional_file(user)?
            {
                config.merge(&layer);
            }
        }

        if let Some(layer) =
            load_optional_file(&self.local)?
        {
            config.merge(&layer);
        }

        config.apply_env();

        Ok(config)
    }

    /// Save config to the user config file.
    ///
    /// Creates the parent directory if it doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::WriteFile`] if the directory cannot be
    /// created or the file cannot be written, or
    /// [`ConfigError::SerializeToml`] if serialization fails.
    pub fn save_user(
        &self,
        config: &RaConfig,
    ) -> Result<(), ConfigError> {
        let path = self
            .user
            .as_deref()
            .ok_or_else(|| ConfigError::WriteFile {
                path: "~/.config/ra/config.toml".to_owned(),
                source: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "could not determine home directory",
                ),
            })?;

        save_config(config, path)
    }

    /// Save config to the local config file.
    ///
    /// Creates the parent directory if it doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::WriteFile`] if the directory cannot be
    /// created or the file cannot be written, or
    /// [`ConfigError::SerializeToml`] if serialization fails.
    pub fn save_local(
        &self,
        config: &RaConfig,
    ) -> Result<(), ConfigError> {
        save_config(config, &self.local)
    }
}

impl Default for ConfigLoader {
    fn default() -> Self {
        Self::new()
    }
}

/// Load a config file if it exists, return None if missing.
fn load_optional_file(
    path: &Path,
) -> Result<Option<RaConfig>, ConfigError> {
    if !path.is_file() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(path).map_err(|e| {
        ConfigError::ReadFile {
            path: path.display().to_string(),
            source: e,
        }
    })?;

    let config: RaConfig =
        toml::from_str(&content).map_err(|e| {
            ConfigError::ParseToml {
                path: path.display().to_string(),
                source: e,
            }
        })?;

    Ok(Some(config))
}

/// Save config to a file, creating parent dirs as needed.
fn save_config(
    config: &RaConfig,
    path: &Path,
) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            ConfigError::WriteFile {
                path: parent.display().to_string(),
                source: e,
            }
        })?;
    }

    let toml_str = config.to_toml()?;

    std::fs::write(path, toml_str).map_err(|e| {
        ConfigError::WriteFile {
            path: path.display().to_string(),
            source: e,
        }
    })
}

/// Get the user's home directory.
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;
    use crate::model::EditorMode;

    #[test]

    fn loader_with_no_files_returns_defaults() {
        let loader = ConfigLoader::new()
            .with_system(PathBuf::from(
                "/nonexistent/system",
            ))
            .with_user(PathBuf::from(
                "/nonexistent/user",
            ))
            .with_local(PathBuf::from(
                "/nonexistent/local",
            ));

        let config = loader.load().expect("load");
        assert_eq!(config, RaConfig::default());
    }

    #[test]

    fn loader_reads_toml_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "[editor]\nmode = \"vi\"\n",
        )
        .expect("write");

        let loader = ConfigLoader::new()
            .with_system(PathBuf::from("/nonexistent"))
            .with_user(path)
            .with_local(PathBuf::from("/nonexistent"));

        let config = loader.load().expect("load");
        assert_eq!(config.editor.mode, EditorMode::Vi);
    }

    #[test]

    fn local_overrides_user() {
        let dir = tempfile::tempdir().expect("tempdir");

        let user = dir.path().join("user.toml");
        std::fs::write(
            &user,
            "[editor]\nmode = \"vi\"\n",
        )
        .expect("write user");

        let local = dir.path().join("local.toml");
        std::fs::write(
            &local,
            "[editor]\nmode = \"nano\"\n",
        )
        .expect("write local");

        let loader = ConfigLoader::new()
            .with_system(PathBuf::from("/nonexistent"))
            .with_user(user)
            .with_local(local);

        let config = loader.load().expect("load");
        assert_eq!(config.editor.mode, EditorMode::Nano);
    }

    #[test]

    fn save_and_reload() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("saved.toml");

        let mut config = RaConfig::default();
        config.editor.mode = EditorMode::Vi;
        config.sql.line_width = 120;

        save_config(&config, &path).expect("save");

        let loaded =
            load_optional_file(&path).expect("load");
        assert_eq!(loaded, Some(config));
    }

    #[test]

    fn save_creates_parent_dirs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path =
            dir.path().join("deep").join("nested").join("config.toml");

        let config = RaConfig::default();
        save_config(&config, &path).expect("save");
        assert!(path.is_file());
    }

    #[test]
    fn config_dir_returns_some() {
        if std::env::var_os("HOME").is_some() {
            assert!(config_dir().is_some());
        }
    }

    #[test]

    fn load_invalid_toml() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bad.toml");
        std::fs::write(&path, "not [valid toml {{{}}")
            .expect("write");

        let loader = ConfigLoader::new()
            .with_system(PathBuf::from("/nonexistent"))
            .with_user(path)
            .with_local(PathBuf::from("/nonexistent"));

        let result = loader.load();
        assert!(result.is_err());
    }
}
