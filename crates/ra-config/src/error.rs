//! Configuration error types.

/// Errors that can occur during configuration loading or access.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// Failed to read a config file.
    #[error("reading config from {path}: {source}")]
    ReadFile {
        /// The file path that failed.
        path: String,
        /// The underlying IO error.
        source: std::io::Error,
    },

    /// Failed to parse TOML content.
    #[error("parsing config from {path}: {source}")]
    ParseToml {
        /// The file path that failed.
        path: String,
        /// The underlying TOML error.
        source: toml::de::Error,
    },

    /// Failed to serialize config to TOML.
    #[error("serializing config: {0}")]
    SerializeToml(#[from] toml::ser::Error),

    /// Failed to write a config file.
    #[error("writing config to {path}: {source}")]
    WriteFile {
        /// The file path that failed.
        path: String,
        /// The underlying IO error.
        source: std::io::Error,
    },

    /// Invalid dotted key path (e.g. "editor.mode").
    #[error("unknown config key: {0}")]
    UnknownKey(String),

    /// Invalid value for a config field.
    #[error("invalid value for {key}: {reason}")]
    InvalidValue {
        /// The config key.
        key: String,
        /// Why the value is invalid.
        reason: String,
    },
}
