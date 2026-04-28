//! The `monitor` subcommand (disabled — ra-pg-monitor crate removed).

use anyhow::{bail, Result};

pub fn cmd_monitor(_tui: bool, _demo: bool, _format: &str, _quiet: bool) -> Result<()> {
    bail!("Monitor command is disabled — ra-pg-monitor crate has been removed from the workspace")
}
