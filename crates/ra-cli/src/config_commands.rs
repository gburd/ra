//! CLI subcommands for managing RA configuration.
//!
//! Provides `config list`, `config get`, `config set`,
//! `config edit`, and `config reset` subcommands.

use anyhow::{Context, Result, bail};
use colored::Colorize;

use ra_config::{ConfigLoader, RaConfig, config_path};

/// Execute `config list` -- show all current settings.
pub fn cmd_config_list(quiet: bool) -> Result<()> {
    let loader = ConfigLoader::new();
    let config = loader
        .load()
        .context("loading configuration")?;

    if quiet {
        for key in RaConfig::keys() {
            if let Ok(val) = config.get(key) {
                eprintln!("{key}={val}");
            }
        }
        return Ok(());
    }

    eprintln!();
    eprintln!("{}", "Configuration".bold());
    eprintln!();

    let max_key_len = RaConfig::keys()
        .iter()
        .map(|k| k.len())
        .max()
        .unwrap_or(0);

    for key in RaConfig::keys() {
        if let Ok(val) = config.get(key) {
            eprintln!(
                "  {:<width$}  {}",
                key.cyan(),
                val,
                width = max_key_len,
            );
        }
    }

    eprintln!();
    if let Some(path) = config_path() {
        eprintln!(
            "  {}: {}",
            "Config file".dimmed(),
            path.display(),
        );
    }
    eprintln!();

    Ok(())
}

/// Execute `config get <key>` -- show a specific setting.
pub fn cmd_config_get(key: &str) -> Result<()> {
    let loader = ConfigLoader::new();
    let config = loader
        .load()
        .context("loading configuration")?;

    let value = config.get(key).map_err(|e| {
        anyhow::anyhow!("{e}\nhint: run 'ra-cli config list' to see available keys")
    })?;

    eprintln!("{value}");
    Ok(())
}

/// Execute `config set <key> <value>` -- update a setting.
pub fn cmd_config_set(
    key: &str,
    value: &str,
    quiet: bool,
) -> Result<()> {
    let loader = ConfigLoader::new();
    let mut config = loader
        .load()
        .context("loading configuration")?;

    config.set(key, value).map_err(|e| {
        anyhow::anyhow!("{e}")
    })?;

    loader
        .save_user(&config)
        .context("saving configuration")?;

    if !quiet {
        eprintln!(
            "{} {} = {}",
            "Set".green().bold(),
            key.cyan(),
            value,
        );
    }

    Ok(())
}

/// Execute `config edit` -- open config in $EDITOR.
pub fn cmd_config_edit() -> Result<()> {
    let path = config_path().ok_or_else(|| {
        anyhow::anyhow!(
            "could not determine config path; \
             set HOME environment variable"
        )
    })?;

    // Ensure the config file exists with current values
    if !path.is_file() {
        let loader = ConfigLoader::new();
        let config = loader
            .load()
            .context("loading configuration")?;
        loader
            .save_user(&config)
            .context("creating initial config file")?;
    }

    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| "vi".to_owned());

    let status = std::process::Command::new(&editor)
        .arg(&path)
        .status()
        .with_context(|| {
            format!("launching editor '{editor}'")
        })?;

    if !status.success() {
        bail!("editor exited with status {status}");
    }

    // Validate the edited config
    let loader = ConfigLoader::new();
    match loader.load() {
        Ok(_) => {
            eprintln!(
                "{}",
                "Configuration saved and validated.".green()
            );
        }
        Err(e) => {
            eprintln!(
                "{}: {e}",
                "Warning: config has errors".yellow().bold()
            );
        }
    }

    Ok(())
}

/// Execute `config reset` -- reset config to defaults.
pub fn cmd_config_reset(quiet: bool) -> Result<()> {
    let config = RaConfig::default();
    let loader = ConfigLoader::new();
    loader
        .save_user(&config)
        .context("saving default configuration")?;

    if !quiet {
        eprintln!(
            "{}",
            "Configuration reset to defaults.".green().bold()
        );
    }

    Ok(())
}

/// Execute `config path` -- show config file location.
pub fn cmd_config_path() -> Result<()> {
    if let Some(path) = config_path() {
        let exists = if path.is_file() { "" } else { " (not created)" };
        eprintln!("{}{exists}", path.display());
    } else {
        bail!(
            "could not determine config path; \
             set HOME environment variable"
        );
    }
    Ok(())
}
