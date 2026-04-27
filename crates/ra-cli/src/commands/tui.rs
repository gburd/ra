//! The `tui` subcommand.

use anyhow::{bail, Context, Result};

pub fn cmd_tui(
    timeline_path: Option<&str>,
    demo: bool,
    headless: bool,
    record_path: Option<&str>,
) -> Result<()> {
    let timeline = if demo {
        ra_tui::Timeline::demo()
    } else if let Some(path) = timeline_path {
        let source = std::fs::read_to_string(path)
            .with_context(|| format!("reading timeline file: {path}"))?;

        if path.ends_with(".json") {
            serde_json::from_str(&source)
                .with_context(|| format!("parsing timeline JSON from: {path}"))?
        } else if path.ends_with(".toml") {
            ra_tui::Timeline::from_toml(&source)
                .map_err(|e| anyhow::anyhow!("{e}"))
                .with_context(|| format!("converting TOML timeline: {path}"))?
        } else {
            serde_json::from_str(&source)
                .with_context(|| format!("parsing timeline from: {path}"))?
        }
    } else {
        bail!(
            "specify --demo for demo data or \
             --timeline <path> to load a timeline file"
        );
    };

    let mut app = ra_tui::App::new(timeline).context("initializing TUI")?;

    if let Some(output) = record_path {
        let path = std::path::Path::new(output);
        let frame_count = ra_tui::record_session(&mut app, path, 120, 40, 1.0)
            .context("recording TUI session")?;
        eprintln!("Recorded {frame_count} frames to {output}");
        return Ok(());
    }

    if headless {
        let final_cost = app.run_headless().context("running headless TUI")?;
        eprintln!("Headless run complete. Final cost: {final_cost:.0}");
        return Ok(());
    }

    app.run().context("running TUI")?;

    Ok(())
}
