//! Layered configuration system for the RA optimizer.
//!
//! Configuration is loaded from multiple sources in priority order
//! (later sources override earlier ones):
//!
//! 1. Built-in defaults
//! 2. System config: `/etc/ra/config.toml`
//! 3. User config: `~/.config/ra/config.toml`
//! 4. Local config: `./.ra/config.toml`
//! 5. Environment variables: `RA_*`
//!
//! The config format is TOML with sections for editor, SQL, TUI,
//! hardware, and output settings.

#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]

mod error;
mod loader;
mod model;

pub use error::ConfigError;
pub use loader::{config_dir, config_path, ConfigLoader};
pub use model::{
    EditorConfig, EditorMode, HardwareConfig, IndentStyle, KeywordCase, OutputConfig, OutputFormat,
    RaConfig, SqlConfig, SqlDialect, TuiConfig, TuiLayout,
};
