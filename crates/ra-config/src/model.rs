//! Configuration data model with typed sections.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::ConfigError;

/// Top-level configuration for the RA optimizer.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RaConfig {
    /// Editor behavior settings.
    #[serde(default)]
    pub editor: EditorConfig,

    /// SQL formatting and dialect settings.
    #[serde(default)]
    pub sql: SqlConfig,

    /// TUI layout and display settings.
    #[serde(default)]
    pub tui: TuiConfig,

    /// Hardware profile selection.
    #[serde(default)]
    pub hardware: HardwareConfig,

    /// Output formatting settings.
    #[serde(default)]
    pub output: OutputConfig,
}

impl RaConfig {
    /// Get a config value by dotted key path.
    ///
    /// Supported keys: `editor.mode`, `editor.theme`, `sql.capitalize`,
    /// `sql.indent_style`, `sql.dialect`, `sql.line_width`,
    /// `tui.layout`, `tui.refresh_rate_ms`, `hardware.profile`,
    /// `output.format`, `output.color`.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::UnknownKey`] if the key is not recognized.
    pub fn get(&self, key: &str) -> Result<String, ConfigError> {
        match key {
            "editor.mode" => Ok(self.editor.mode.to_string()),
            "editor.theme" => Ok(self.editor.theme.clone()),
            "sql.capitalize" => Ok(self.sql.capitalize.to_string()),
            "sql.indent_style" => Ok(self.sql.indent_style.to_string()),
            "sql.dialect" => Ok(self.sql.dialect.to_string()),
            "sql.line_width" => Ok(self.sql.line_width.to_string()),
            "tui.layout" => Ok(self.tui.layout.to_string()),
            "tui.refresh_rate_ms" => Ok(self.tui.refresh_rate_ms.to_string()),
            "hardware.profile" => Ok(self.hardware.profile.clone()),
            "output.format" => Ok(self.output.format.to_string()),
            "output.color" => Ok(self.output.color.to_string()),
            _ => Err(ConfigError::UnknownKey(key.to_owned())),
        }
    }

    /// Set a config value by dotted key path.
    ///
    /// Validates the value before setting it.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::UnknownKey`] if the key is not recognized,
    /// or [`ConfigError::InvalidValue`] if the value is not valid for
    /// the given key.
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), ConfigError> {
        match key {
            "editor.mode" => {
                self.editor.mode = parse_enum(key, value)?;
            }
            "editor.theme" => {
                value.clone_into(&mut self.editor.theme);
            }
            "sql.capitalize" => {
                self.sql.capitalize = parse_enum(key, value)?;
            }
            "sql.indent_style" => {
                self.sql.indent_style = parse_enum(key, value)?;
            }
            "sql.dialect" => {
                self.sql.dialect = parse_enum(key, value)?;
            }
            "sql.line_width" => {
                self.sql.line_width = value.parse().map_err(|_| ConfigError::InvalidValue {
                    key: key.to_owned(),
                    reason: format!("expected integer, got '{value}'"),
                })?;
            }
            "tui.layout" => {
                self.tui.layout = parse_enum(key, value)?;
            }
            "tui.refresh_rate_ms" => {
                self.tui.refresh_rate_ms =
                    value.parse().map_err(|_| ConfigError::InvalidValue {
                        key: key.to_owned(),
                        reason: format!("expected integer, got '{value}'"),
                    })?;
            }
            "hardware.profile" => {
                value.clone_into(&mut self.hardware.profile);
            }
            "output.format" => {
                self.output.format = parse_enum(key, value)?;
            }
            "output.color" => {
                self.output.color = value.parse().map_err(|_| ConfigError::InvalidValue {
                    key: key.to_owned(),
                    reason: format!("expected true/false, got '{value}'"),
                })?;
            }
            _ => return Err(ConfigError::UnknownKey(key.to_owned())),
        }
        Ok(())
    }

    /// Return all known config keys.
    #[must_use]
    pub fn keys() -> &'static [&'static str] {
        &[
            "editor.mode",
            "editor.theme",
            "sql.capitalize",
            "sql.indent_style",
            "sql.dialect",
            "sql.line_width",
            "tui.layout",
            "tui.refresh_rate_ms",
            "hardware.profile",
            "output.format",
            "output.color",
        ]
    }

    /// Merge non-default values from `other` into `self`.
    ///
    /// Only overrides fields where `other` differs from the default.
    pub fn merge(&mut self, other: &Self) {
        let defaults = Self::default();
        if other.editor != defaults.editor {
            self.editor = other.editor.clone();
        }
        if other.sql != defaults.sql {
            self.sql = other.sql.clone();
        }
        if other.tui != defaults.tui {
            self.tui = other.tui.clone();
        }
        if other.hardware != defaults.hardware {
            self.hardware = other.hardware.clone();
        }
        if other.output != defaults.output {
            self.output = other.output.clone();
        }
    }

    /// Apply environment variable overrides.
    ///
    /// Reads `RA_EDITOR_MODE`, `RA_SQL_DIALECT`, etc. and applies
    /// them on top of the current config. Invalid values are
    /// silently ignored.
    pub fn apply_env(&mut self) {
        let env_mappings: &[(&str, &str)] = &[
            ("RA_EDITOR_MODE", "editor.mode"),
            ("RA_EDITOR_THEME", "editor.theme"),
            ("RA_SQL_CAPITALIZE", "sql.capitalize"),
            ("RA_SQL_INDENT_STYLE", "sql.indent_style"),
            ("RA_SQL_DIALECT", "sql.dialect"),
            ("RA_SQL_LINE_WIDTH", "sql.line_width"),
            ("RA_TUI_LAYOUT", "tui.layout"),
            ("RA_TUI_REFRESH_RATE_MS", "tui.refresh_rate_ms"),
            ("RA_HARDWARE_PROFILE", "hardware.profile"),
            ("RA_OUTPUT_FORMAT", "output.format"),
            ("RA_OUTPUT_COLOR", "output.color"),
        ];

        for (env_var, config_key) in env_mappings {
            if let Ok(val) = std::env::var(env_var) {
                let _ = self.set(config_key, &val);
            }
        }
    }

    /// Serialize to TOML string.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::SerializeToml`] if serialization fails.
    pub fn to_toml(&self) -> Result<String, ConfigError> {
        toml::to_string_pretty(self).map_err(ConfigError::SerializeToml)
    }
}

/// Parse a string into an enum that implements `ParseFromStr`.
fn parse_enum<T: ParseFromStr>(key: &str, value: &str) -> Result<T, ConfigError> {
    T::parse_from_str(value).ok_or_else(|| ConfigError::InvalidValue {
        key: key.to_owned(),
        reason: format!(
            "unknown value '{value}'; valid options: {}",
            T::valid_options()
        ),
    })
}

/// Trait for types that can be parsed from a string config value.
trait ParseFromStr: Sized {
    /// Try to parse from a string.
    fn parse_from_str(s: &str) -> Option<Self>;
    /// Comma-separated list of valid options.
    fn valid_options() -> &'static str;
}

// ── Editor config ───────────────────────────────────────────

/// Editor behavior settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EditorConfig {
    /// Keybinding mode.
    #[serde(default)]
    pub mode: EditorMode,

    /// Color theme name.
    #[serde(default = "default_theme")]
    pub theme: String,
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            mode: EditorMode::default(),
            theme: default_theme(),
        }
    }
}

fn default_theme() -> String {
    "default".to_owned()
}

/// Editor keybinding mode.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EditorMode {
    /// Standard editor mode.
    #[default]
    Normal,
    /// Vi-style keybindings.
    Vi,
    /// Nano-style keybindings.
    Nano,
}

impl fmt::Display for EditorMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Normal => write!(f, "normal"),
            Self::Vi => write!(f, "vi"),
            Self::Nano => write!(f, "nano"),
        }
    }
}

impl ParseFromStr for EditorMode {
    fn parse_from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "normal" | "default" => Some(Self::Normal),
            "vi" | "vim" => Some(Self::Vi),
            "nano" | "emacs" => Some(Self::Nano),
            _ => None,
        }
    }

    fn valid_options() -> &'static str {
        "normal, vi, nano"
    }
}

// ── SQL config ──────────────────────────────────────────────

/// SQL formatting and dialect settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SqlConfig {
    /// Keyword capitalization style.
    #[serde(default)]
    pub capitalize: KeywordCase,

    /// Indentation style.
    #[serde(default)]
    pub indent_style: IndentStyle,

    /// SQL dialect for parser features.
    #[serde(default)]
    pub dialect: SqlDialect,

    /// Maximum line width for formatting.
    #[serde(default = "default_line_width")]
    pub line_width: u32,
}

impl Default for SqlConfig {
    fn default() -> Self {
        Self {
            capitalize: KeywordCase::default(),
            indent_style: IndentStyle::default(),
            dialect: SqlDialect::default(),
            line_width: default_line_width(),
        }
    }
}

fn default_line_width() -> u32 {
    80
}

/// SQL keyword capitalization.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum KeywordCase {
    /// Keep keywords as written.
    Preserve,
    /// Uppercase keywords (SELECT, FROM, WHERE).
    #[default]
    Keywords,
    /// Uppercase everything.
    Upper,
    /// Lowercase everything.
    Lower,
}

impl fmt::Display for KeywordCase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Preserve => write!(f, "preserve"),
            Self::Keywords => write!(f, "keywords"),
            Self::Upper => write!(f, "upper"),
            Self::Lower => write!(f, "lower"),
        }
    }
}

impl ParseFromStr for KeywordCase {
    fn parse_from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "preserve" | "none" => Some(Self::Preserve),
            "keywords" | "keyword" => Some(Self::Keywords),
            "upper" | "uppercase" => Some(Self::Upper),
            "lower" | "lowercase" => Some(Self::Lower),
            _ => None,
        }
    }

    fn valid_options() -> &'static str {
        "preserve, keywords, upper, lower"
    }
}

/// Indentation style.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IndentStyle {
    /// Two spaces.
    #[default]
    #[serde(rename = "2space")]
    TwoSpace,
    /// Four spaces.
    #[serde(rename = "4space")]
    FourSpace,
    /// Tab character.
    Tab,
}

impl fmt::Display for IndentStyle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TwoSpace => write!(f, "2space"),
            Self::FourSpace => write!(f, "4space"),
            Self::Tab => write!(f, "tab"),
        }
    }
}

impl ParseFromStr for IndentStyle {
    fn parse_from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "2space" | "2" | "two" => Some(Self::TwoSpace),
            "4space" | "4" | "four" => Some(Self::FourSpace),
            "tab" | "tabs" => Some(Self::Tab),
            _ => None,
        }
    }

    fn valid_options() -> &'static str {
        "2space, 4space, tab"
    }
}

/// SQL dialect for parser features.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SqlDialect {
    /// ANSI SQL standard.
    #[default]
    Ansi,
    /// `PostgreSQL` extensions.
    #[serde(alias = "postgres")]
    Postgresql,
    /// `MySQL` extensions.
    Mysql,
    /// `SQLite` extensions.
    Sqlite,
    /// Oracle extensions.
    Oracle,
    /// SQL Server extensions.
    #[serde(alias = "mssql")]
    Sqlserver,
}

impl fmt::Display for SqlDialect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ansi => write!(f, "ansi"),
            Self::Postgresql => write!(f, "postgresql"),
            Self::Mysql => write!(f, "mysql"),
            Self::Sqlite => write!(f, "sqlite"),
            Self::Oracle => write!(f, "oracle"),
            Self::Sqlserver => write!(f, "sqlserver"),
        }
    }
}

impl ParseFromStr for SqlDialect {
    fn parse_from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "ansi" | "standard" => Some(Self::Ansi),
            "postgresql" | "postgres" | "pg" => Some(Self::Postgresql),
            "mysql" => Some(Self::Mysql),
            "sqlite" => Some(Self::Sqlite),
            "oracle" => Some(Self::Oracle),
            "sqlserver" | "mssql" | "tsql" => Some(Self::Sqlserver),
            _ => None,
        }
    }

    fn valid_options() -> &'static str {
        "ansi, postgresql, mysql, sqlite, oracle, sqlserver"
    }
}

// ── TUI config ──────────────────────────────────────────────

/// TUI layout and display settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TuiConfig {
    /// Default panel layout.
    #[serde(default)]
    pub layout: TuiLayout,

    /// Event loop refresh rate in milliseconds.
    #[serde(default = "default_refresh_rate")]
    pub refresh_rate_ms: u32,
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            layout: TuiLayout::default(),
            refresh_rate_ms: default_refresh_rate(),
        }
    }
}

fn default_refresh_rate() -> u32 {
    100
}

/// TUI panel layout presets.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TuiLayout {
    /// Standard 4-panel editor layout.
    #[default]
    Editor,
    /// Focus on plan visualization.
    Plan,
    /// Focus on statistics.
    Stats,
    /// Compact single-panel mode.
    Compact,
}

impl fmt::Display for TuiLayout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Editor => write!(f, "editor"),
            Self::Plan => write!(f, "plan"),
            Self::Stats => write!(f, "stats"),
            Self::Compact => write!(f, "compact"),
        }
    }
}

impl ParseFromStr for TuiLayout {
    fn parse_from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "editor" | "default" => Some(Self::Editor),
            "plan" | "tree" => Some(Self::Plan),
            "stats" | "statistics" => Some(Self::Stats),
            "compact" | "minimal" => Some(Self::Compact),
            _ => None,
        }
    }

    fn valid_options() -> &'static str {
        "editor, plan, stats, compact"
    }
}

// ── Hardware config ─────────────────────────────────────────

/// Hardware profile selection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HardwareConfig {
    /// Hardware profile name (auto, cpu-only, gpu-server, fpga).
    #[serde(default = "default_hardware_profile")]
    pub profile: String,
}

impl Default for HardwareConfig {
    fn default() -> Self {
        Self {
            profile: default_hardware_profile(),
        }
    }
}

fn default_hardware_profile() -> String {
    "auto".to_owned()
}

// ── Output config ───────────────────────────────────────────

/// Output formatting settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OutputConfig {
    /// Default output format.
    #[serde(default)]
    pub format: OutputFormat,

    /// Enable colored output.
    #[serde(default = "default_color")]
    pub color: bool,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            format: OutputFormat::default(),
            color: default_color(),
        }
    }
}

fn default_color() -> bool {
    true
}

/// Output format for CLI commands.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    /// Human-readable text.
    #[default]
    Text,
    /// JSON output.
    Json,
    /// Compact summary.
    Compact,
}

impl fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text => write!(f, "text"),
            Self::Json => write!(f, "json"),
            Self::Compact => write!(f, "compact"),
        }
    }
}

impl ParseFromStr for OutputFormat {
    fn parse_from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "text" | "human" | "pretty" => Some(Self::Text),
            "json" => Some(Self::Json),
            "compact" | "summary" => Some(Self::Compact),
            _ => None,
        }
    }

    fn valid_options() -> &'static str {
        "text, json, compact"
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;

    #[test]
    fn default_config_roundtrips_through_toml() {
        let config = RaConfig::default();
        let toml_str = toml::to_string_pretty(&config).expect("serialize");
        let parsed: RaConfig = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(config, parsed);
    }

    #[test]
    fn get_all_keys() {
        let config = RaConfig::default();
        for key in RaConfig::keys() {
            let val = config.get(key);
            assert!(val.is_ok(), "get({key}) failed: {val:?}");
        }
    }

    #[test]
    fn get_unknown_key() {
        let config = RaConfig::default();
        assert!(config.get("unknown.key").is_err());
    }

    #[test]
    fn set_editor_mode() {
        let mut config = RaConfig::default();
        config.set("editor.mode", "vi").expect("set vi");
        assert_eq!(config.editor.mode, EditorMode::Vi);
        assert_eq!(config.get("editor.mode").expect("get"), "vi");
    }

    #[test]
    fn set_sql_line_width() {
        let mut config = RaConfig::default();
        config.set("sql.line_width", "120").expect("set width");
        assert_eq!(config.sql.line_width, 120);
    }

    #[test]
    fn set_invalid_value() {
        let mut config = RaConfig::default();
        let result = config.set("editor.mode", "emacs-lisp");
        assert!(result.is_err());
    }

    #[test]
    fn set_invalid_integer() {
        let mut config = RaConfig::default();
        let result = config.set("sql.line_width", "abc");
        assert!(result.is_err());
    }

    #[test]
    fn merge_overrides_non_default() {
        let mut base = RaConfig::default();
        let mut overlay = RaConfig::default();
        overlay.editor.mode = EditorMode::Vi;
        base.merge(&overlay);
        assert_eq!(base.editor.mode, EditorMode::Vi);
    }

    #[test]
    fn merge_preserves_base_when_overlay_is_default() {
        let mut base = RaConfig::default();
        base.editor.mode = EditorMode::Vi;
        let overlay = RaConfig::default();
        base.merge(&overlay);
        assert_eq!(base.editor.mode, EditorMode::Vi);
    }

    #[test]
    fn editor_mode_display() {
        assert_eq!(EditorMode::Normal.to_string(), "normal");
        assert_eq!(EditorMode::Vi.to_string(), "vi");
        assert_eq!(EditorMode::Nano.to_string(), "nano");
    }

    #[test]
    fn sql_dialect_parse() {
        assert_eq!(
            SqlDialect::parse_from_str("pg"),
            Some(SqlDialect::Postgresql)
        );
        assert_eq!(
            SqlDialect::parse_from_str("mssql"),
            Some(SqlDialect::Sqlserver)
        );
        assert!(SqlDialect::parse_from_str("xyz").is_none());
    }

    #[test]
    fn indent_style_roundtrip() {
        for style in [
            IndentStyle::TwoSpace,
            IndentStyle::FourSpace,
            IndentStyle::Tab,
        ] {
            let s = style.to_string();
            let parsed = IndentStyle::parse_from_str(&s);
            assert_eq!(parsed, Some(style));
        }
    }

    #[test]
    fn to_toml_produces_valid_output() {
        let config = RaConfig::default();
        let toml_str = config.to_toml().expect("to_toml");
        assert!(toml_str.contains("[editor]"));
        assert!(toml_str.contains("[sql]"));
    }

    #[test]
    fn partial_toml_deserializes_with_defaults() {
        let partial = r#"
[editor]
mode = "vi"
"#;
        let config: RaConfig = toml::from_str(partial).expect("parse");
        assert_eq!(config.editor.mode, EditorMode::Vi);
        assert_eq!(config.sql.line_width, 80);
    }

    #[test]
    fn env_override_applies() {
        let mut config = RaConfig::default();
        std::env::set_var("RA_EDITOR_MODE", "vi");
        config.apply_env();
        assert_eq!(config.editor.mode, EditorMode::Vi);
        std::env::remove_var("RA_EDITOR_MODE");
    }
}
