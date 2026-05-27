//! Layered configuration system for the RA optimizer.
//!
//! # History
//!
//! `ra-config` was the original home of the configuration layer. It has
//! since been merged into [`ra_core::config`] so a single dependency
//! covers both the algebra types and the configuration model. This
//! crate is preserved as a thin re-export shim for downstream consumers
//! that depended on the `ra_config::*` import path; new code should
//! import directly from [`ra_core::config`].
//!
//! Configuration is loaded from multiple sources in priority order
//! (later sources override earlier ones):
//!
//! 1. Built-in defaults
//! 2. System config: `/etc/ra/config.toml`
//! 3. User config: `~/.config/ra/config.toml`
//! 4. Local config: `./.ra/config.toml`
//! 5. Environment variables: `RA_*`

#![warn(missing_docs)]

pub use ra_core::config::{
    config_dir, config_path, ConfigError, ConfigLoader, ConfigSqlDialect as SqlDialect,
    EditorConfig, EditorMode, HardwareConfig, IndentStyle, KeywordCase, OutputConfig,
    OutputFormat, RaConfig, SqlConfig, TuiConfig, TuiLayout,
};
