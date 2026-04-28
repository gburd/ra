//! The `tui` subcommand (disabled — ra-tui crate removed).

use anyhow::{bail, Result};

pub fn cmd_tui(
    _timeline_path: Option<&str>,
    _demo: bool,
    _headless: bool,
    _record_path: Option<&str>,
) -> Result<()> {
    bail!("TUI command is disabled — ra-tui crate has been removed from the workspace")
}
