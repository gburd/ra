//! CLI commands for statistics timeline interaction.
//!
//! Provides three subcommands:
//! - `stats play` - Replay timeline with streaming output
//! - `stats feedback` - Simulate batch execution with feedback
//! - `stats visualize` - Generate cost/cardinality evolution charts

#![allow(clippy::print_stderr)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::unnecessary_wraps)]
#![allow(clippy::uninlined_format_args)]

use std::path::Path;
use std::thread;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use colored::Colorize;

use ra_stats::timeline::{EventKind, ExecutionFeedback, Snapshot, Timeline, TimelinePlayer};

use crate::visualize::{
    infer_change_reason, operators_differ, render_ascii_bar_chart, render_ascii_sparkline,
    render_ascii_table, render_html_chart, render_plan_evolution_ascii, ChartConfig, DataPoint,
    PlanEvolutionTrace, PlanSnapshot, Series,
};

/// Output format for stats commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable table format.
    Table,
    /// Machine-readable JSON.
    Json,
    /// ASCII chart (bar/sparkline).
    Ascii,
    /// HTML with SVG charts.
    Html,
}

impl OutputFormat {
    /// Parse from CLI string argument.
    pub fn from_str_arg(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "table" => Ok(Self::Table),
            "json" => Ok(Self::Json),
            "ascii" => Ok(Self::Ascii),
            "html" => Ok(Self::Html),
            other => bail!(
                "unknown format: {other}\n\
                 valid formats: table, json, ascii, html"
            ),
        }
    }
}

/// Load a timeline from a TOML file path.
pub fn load_timeline(path: &str) -> Result<Timeline> {
    let p = Path::new(path);
    if !p.exists() {
        bail!(
            "timeline file not found: {path}\n\
             hint: check the path or use one from timelines/"
        );
    }
    let content =
        std::fs::read_to_string(p).with_context(|| format!("reading timeline: {path}"))?;
    Timeline::from_toml(&content).map_err(|e| anyhow::anyhow!("parsing timeline: {e}"))
}

// -- stats play --

/// Execute the `stats play` command.
pub fn cmd_stats_play(
    timeline_path: &str,
    format: OutputFormat,
    speed: f64,
    verbose: bool,
) -> Result<()> {
    let timeline = load_timeline(timeline_path)?;
    let mut player = TimelinePlayer::new(timeline).map_err(|e| anyhow::anyhow!("{e}"))?;

    match format {
        OutputFormat::Json => play_json(&mut player, speed),
        OutputFormat::Table => play_table(&mut player, speed, verbose),
        OutputFormat::Ascii => play_ascii(&mut player),
        OutputFormat::Html => play_html(&player),
    }
}

fn play_table(player: &mut TimelinePlayer, speed: f64, verbose: bool) -> Result<()> {
    let tl = player.timeline();
    eprintln!();
    eprintln!(
        "{}",
        format!(
            "Timeline: {} ({} snapshots, {} events)",
            tl.metadata.name,
            tl.snapshot_count(),
            tl.event_count(),
        )
        .bold()
    );
    eprintln!("  {}: {}", "Description".bold(), tl.metadata.description,);
    if let Some(db) = &tl.metadata.database {
        eprintln!("  {}: {db}", "Database".bold());
    }
    if let Some(schema) = &tl.metadata.schema {
        eprintln!("  {}: {schema}", "Schema".bold());
    }
    eprintln!("  {}: {:.1}x", "Playback speed".bold(), speed,);
    eprintln!();

    let delay_base = if speed > 0.0 {
        (1000.0 / speed) as u64
    } else {
        0
    };

    let snap_count = player.snapshot_count();
    player.seek_start();

    for idx in 0..snap_count {
        if idx > 0 {
            if let Err(e) = player.seek(idx) {
                bail!("seek failed: {e}");
            }
        }

        let snap = player
            .current_snapshot()
            .context("no snapshot at position")?;

        eprintln!(
            "{} Snapshot {}/{} (t={}s{})",
            "[PLAY]".cyan().bold(),
            idx + 1,
            snap_count,
            snap.time_offset,
            snap.label
                .as_ref()
                .map_or(String::new(), |l| format!(" - {l}")),
        );

        print_snapshot_table(snap);

        // Show events until next snapshot
        let events = player.events_until_next();
        if !events.is_empty() {
            eprintln!("  {}:", "Events".bold());
            for event in &events {
                eprintln!(
                    "    t={}: {} on {} {}",
                    event.time_offset,
                    format_event_kind(&event.kind).yellow(),
                    event.table,
                    event
                        .row_count
                        .map_or(String::new(), |r| format!("({r} rows)")),
                );
            }
        }

        // Show feedback at current time
        let feedback = player.feedback_at_current();
        if !feedback.is_empty() && verbose {
            eprintln!("  {}:", "Feedback".bold());
            for fb in &feedback {
                print_feedback_entry(fb);
            }
        }

        eprintln!();

        // Simulate time delay between snapshots
        if delay_base > 0 && idx + 1 < snap_count {
            thread::sleep(Duration::from_millis(delay_base));
        }
    }

    // Summary
    if let Some(avg_q) = player.average_q_error() {
        eprintln!(
            "{} Average Q-error: {:.2}, Max: {:.2}",
            "[DONE]".green().bold(),
            avg_q,
            player.max_q_error().unwrap_or(0.0),
        );
    } else {
        eprintln!("{} Playback complete.", "[DONE]".green().bold());
    }

    Ok(())
}

fn play_json(player: &mut TimelinePlayer, _speed: f64) -> Result<()> {
    let tl = player.timeline();
    let json = serde_json::to_string_pretty(tl).context("serializing timeline to JSON")?;
    eprintln!("{json}");
    Ok(())
}

fn play_ascii(player: &mut TimelinePlayer) -> Result<()> {
    let tl = player.timeline();
    let table_names = tl.table_names();

    for table_name in &table_names {
        let series = build_row_count_series(tl, table_name);
        let config = ChartConfig {
            title: format!("{table_name} - Row Count Evolution"),
            y_label: "Rows".to_owned(),
            x_label: "Time (s)".to_owned(),
            width: 60,
            height: 15,
        };
        eprintln!("{}", render_ascii_bar_chart(&series, &config));
    }

    // Q-error sparkline if feedback exists
    if !tl.feedback.is_empty() {
        let q_series = build_q_error_series(tl);
        let config = ChartConfig {
            title: "Q-Error Over Time".to_owned(),
            y_label: "Q-Error".to_owned(),
            x_label: "Time (s)".to_owned(),
            ..ChartConfig::default()
        };
        eprintln!("{}", render_ascii_sparkline(&[q_series], &config));
    }

    Ok(())
}

fn play_html(player: &TimelinePlayer) -> Result<()> {
    let tl = player.timeline();
    let mut all_series = Vec::new();

    for table_name in &tl.table_names() {
        all_series.push(build_row_count_series(tl, table_name));
    }

    let config = ChartConfig {
        title: format!("{} - Statistics Evolution", tl.metadata.name,),
        y_label: "Row Count".to_owned(),
        x_label: "Time (s)".to_owned(),
        width: 800,
        height: 400,
    };

    eprintln!("{}", render_html_chart(&all_series, &config));
    Ok(())
}

// -- stats feedback --

/// Execute the `stats feedback` command.
pub fn cmd_stats_feedback(
    timeline_path: &str,
    format: OutputFormat,
    batch_size: usize,
    verbose: bool,
) -> Result<()> {
    let timeline = load_timeline(timeline_path)?;
    let mut player = TimelinePlayer::new(timeline).map_err(|e| anyhow::anyhow!("{e}"))?;

    let tl = player.timeline().clone();

    match format {
        OutputFormat::Json => feedback_json(&tl),
        OutputFormat::Table => feedback_table(&mut player, &tl, batch_size, verbose),
        OutputFormat::Ascii => feedback_ascii(&tl),
        OutputFormat::Html => feedback_html(&tl),
    }
}

fn feedback_table(
    player: &mut TimelinePlayer,
    tl: &Timeline,
    batch_size: usize,
    verbose: bool,
) -> Result<()> {
    if tl.feedback.is_empty() {
        eprintln!("{}", "No execution feedback in this timeline.".dimmed());
        return Ok(());
    }

    eprintln!();
    eprintln!(
        "{}",
        format!(
            "Feedback Simulation: {} ({} entries, batch size {})",
            tl.metadata.name,
            tl.feedback.len(),
            batch_size,
        )
        .bold()
    );
    eprintln!();

    let snap_count = player.snapshot_count();
    player.seek_start();

    // Process feedback in batches
    let chunks: Vec<&[ExecutionFeedback]> = tl.feedback.chunks(batch_size).collect();

    for (batch_idx, chunk) in chunks.iter().enumerate() {
        eprintln!(
            "{} Batch {}/{} ({} entries)",
            "[BATCH]".cyan().bold(),
            batch_idx + 1,
            chunks.len(),
            chunk.len(),
        );

        let mut headers = vec![
            "Time",
            "Query",
            "Estimated",
            "Actual",
            "Q-Error",
            "Direction",
        ];
        if verbose {
            headers.push("Operator");
        }

        let mut rows = Vec::new();
        for fb in *chunk {
            let direction = if fb.is_overestimate() {
                "OVER"
            } else if fb.is_underestimate() {
                "UNDER"
            } else {
                "EXACT"
            };
            let query_short = truncate_query(&fb.query, 30);
            let mut row = vec![
                format!("t={}", fb.time_offset),
                query_short,
                format!("{:.0}", fb.estimated_rows),
                format!("{:.0}", fb.actual_rows),
                format!("{:.2}", fb.q_error()),
                direction.to_owned(),
            ];
            if verbose {
                row.push(fb.operator.as_deref().unwrap_or("-").to_owned());
            }
            rows.push(row);
        }

        let header_refs = headers.to_vec();
        eprintln!("{}", render_ascii_table(&header_refs, &rows));

        // Show batch statistics
        let batch_q_errors: Vec<f64> = chunk.iter().map(ExecutionFeedback::q_error).collect();
        let avg_q: f64 = batch_q_errors.iter().sum::<f64>() / batch_q_errors.len() as f64;
        let max_q = batch_q_errors
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        let overestimates = chunk.iter().filter(|f| f.is_overestimate()).count();
        let underestimates = chunk.iter().filter(|f| f.is_underestimate()).count();

        eprintln!(
            "  Avg Q-error: {:.2}, Max: {:.2}, Over: {}, Under: {}",
            avg_q, max_q, overestimates, underestimates,
        );

        // Check if reoptimization was triggered
        let batch_start = chunk.first().map_or(0, |f| f.time_offset);
        let batch_end = chunk.last().map_or(u64::MAX, |f| f.time_offset);
        let reopt_count = tl
            .events_in_range(batch_start, batch_end)
            .iter()
            .filter(|e| e.kind == EventKind::Reoptimize)
            .count();
        if reopt_count > 0 {
            eprintln!(
                "  {} Reoptimization triggered ({} events)",
                ">>".yellow().bold(),
                reopt_count,
            );
        }

        eprintln!();
    }

    // Overall summary
    let all_q: Vec<f64> = tl.feedback.iter().map(ExecutionFeedback::q_error).collect();
    let total_avg = all_q.iter().sum::<f64>() / all_q.len() as f64;
    let total_max = all_q.iter().copied().fold(f64::NEG_INFINITY, f64::max);

    eprintln!(
        "{} Overall: Avg Q-error: {:.2}, Max: {:.2}, Entries: {}",
        "[DONE]".green().bold(),
        total_avg,
        total_max,
        tl.feedback.len(),
    );

    // Show snapshot progression if verbose
    if verbose && snap_count > 1 {
        eprintln!();
        eprintln!("{}", "Snapshot Progression:".bold());
        for table_name in &tl.table_names() {
            let first_rows = tl
                .snapshots
                .first()
                .and_then(|s| s.tables.iter().find(|t| t.name == *table_name))
                .map(|t| t.row_count);
            let last_rows = tl
                .snapshots
                .last()
                .and_then(|s| s.tables.iter().find(|t| t.name == *table_name))
                .map(|t| t.row_count);

            if let (Some(first), Some(last)) = (first_rows, last_rows) {
                let delta = last as i64 - first as i64;
                let sign = if delta >= 0 { "+" } else { "" };
                eprintln!("  {}: {} -> {} ({sign}{delta})", table_name, first, last,);
            }
        }
    }

    Ok(())
}

fn feedback_json(tl: &Timeline) -> Result<()> {
    let entries: Vec<serde_json::Value> = tl
        .feedback
        .iter()
        .map(|fb| {
            serde_json::json!({
                "time_offset": fb.time_offset,
                "query": fb.query,
                "operator": fb.operator,
                "estimated_rows": fb.estimated_rows,
                "actual_rows": fb.actual_rows,
                "q_error": fb.q_error(),
                "direction": if fb.is_overestimate() {
                    "overestimate"
                } else if fb.is_underestimate() {
                    "underestimate"
                } else {
                    "exact"
                },
                "estimated_cost": fb.estimated_cost,
                "actual_time_ms": fb.actual_time_ms,
            })
        })
        .collect();

    let json = serde_json::to_string_pretty(&entries).context("serializing feedback to JSON")?;
    eprintln!("{json}");
    Ok(())
}

fn feedback_ascii(tl: &Timeline) -> Result<()> {
    if tl.feedback.is_empty() {
        eprintln!("{}", "No execution feedback in this timeline.".dimmed());
        return Ok(());
    }

    let q_series = build_q_error_series(tl);
    let est_series = build_estimated_series(tl);
    let act_series = build_actual_series(tl);

    let config = ChartConfig {
        title: format!("{} - Q-Error", tl.metadata.name),
        y_label: "Q-Error".to_owned(),
        x_label: "Time (s)".to_owned(),
        width: 60,
        height: 15,
    };
    eprintln!("{}", render_ascii_bar_chart(&q_series, &config));

    let config2 = ChartConfig {
        title: "Estimated vs Actual Rows".to_owned(),
        y_label: "Rows".to_owned(),
        x_label: "Time (s)".to_owned(),
        ..ChartConfig::default()
    };
    eprintln!(
        "{}",
        render_ascii_sparkline(&[est_series, act_series], &config2)
    );

    Ok(())
}

fn feedback_html(tl: &Timeline) -> Result<()> {
    if tl.feedback.is_empty() {
        eprintln!("{}", "No execution feedback in this timeline.".dimmed());
        return Ok(());
    }

    let est_series = build_estimated_series(tl);
    let act_series = build_actual_series(tl);
    let q_series = build_q_error_series(tl);

    let config = ChartConfig {
        title: format!("{} - Estimation Accuracy", tl.metadata.name,),
        y_label: "Rows".to_owned(),
        x_label: "Time (s)".to_owned(),
        width: 800,
        height: 400,
    };

    eprintln!(
        "{}",
        render_html_chart(&[est_series, act_series, q_series], &config)
    );
    Ok(())
}

// -- stats visualize --

/// Execute the `stats visualize` command.
pub fn cmd_stats_visualize(timeline_path: &str, format: OutputFormat, verbose: bool) -> Result<()> {
    let timeline = load_timeline(timeline_path)?;
    let player = TimelinePlayer::new(timeline).map_err(|e| anyhow::anyhow!("{e}"))?;

    let tl = player.timeline();

    match format {
        OutputFormat::Ascii => visualize_ascii(tl, &player, verbose),
        OutputFormat::Html => visualize_html(tl, &player),
        OutputFormat::Table => visualize_table(tl, &player, verbose),
        OutputFormat::Json => visualize_json(tl, &player),
    }
}

fn visualize_table(tl: &Timeline, player: &TimelinePlayer, verbose: bool) -> Result<()> {
    eprintln!();
    eprintln!(
        "{}",
        format!("Timeline Visualization: {}", tl.metadata.name,).bold()
    );
    eprintln!();

    // Snapshot summary table
    let mut headers = vec!["#", "Time", "Label"];
    let table_names = tl.table_names();
    for name in &table_names {
        headers.push(name);
    }

    let mut rows = Vec::new();
    for (i, snap) in tl.snapshots.iter().enumerate() {
        let mut row = vec![
            format!("{}", i + 1),
            format!("t={}s", snap.time_offset),
            snap.label.clone().unwrap_or_default(),
        ];
        for name in &table_names {
            let rows_str = snap
                .tables
                .iter()
                .find(|t| t.name == *name)
                .map_or("-".to_owned(), |t| format_number(t.row_count));
            row.push(rows_str);
        }
        rows.push(row);
    }

    eprintln!("{}", render_ascii_table(&headers, &rows));

    // Events table
    if !tl.events.is_empty() {
        eprintln!("{}", "Events:".bold());
        let evt_headers = vec!["Time", "Kind", "Table", "Rows", "Description"];
        let evt_rows: Vec<Vec<String>> = tl
            .events
            .iter()
            .map(|e| {
                vec![
                    format!("t={}s", e.time_offset),
                    format_event_kind(&e.kind),
                    e.table.clone(),
                    e.row_count.map_or("-".to_owned(), format_number),
                    e.description.clone().unwrap_or_default(),
                ]
            })
            .collect();
        eprintln!("{}", render_ascii_table(&evt_headers, &evt_rows));
    }

    // Feedback summary
    if !tl.feedback.is_empty() {
        eprintln!("{}", "Feedback Summary:".bold());
        if let Some(avg_q) = player.average_q_error() {
            eprintln!("  Average Q-error: {avg_q:.2}");
        }
        if let Some(max_q) = player.max_q_error() {
            eprintln!("  Max Q-error:     {max_q:.2}");
        }
        eprintln!("  Total entries:   {}", tl.feedback.len());

        if verbose {
            eprintln!();
            let fb_headers = vec!["Time", "Est.", "Actual", "Q-Error", "Dir."];
            let fb_rows: Vec<Vec<String>> = tl
                .feedback
                .iter()
                .map(|fb| {
                    vec![
                        format!("t={}s", fb.time_offset),
                        format!("{:.0}", fb.estimated_rows),
                        format!("{:.0}", fb.actual_rows),
                        format!("{:.2}", fb.q_error()),
                        if fb.is_overestimate() {
                            "OVER".to_owned()
                        } else if fb.is_underestimate() {
                            "UNDER".to_owned()
                        } else {
                            "EXACT".to_owned()
                        },
                    ]
                })
                .collect();
            eprintln!("{}", render_ascii_table(&fb_headers, &fb_rows));
        }

        // Plan evolution
        let traces = build_plan_evolution_traces(tl);
        if !traces.is_empty() {
            eprintln!();
            eprintln!("{}", "Plan Evolution:".bold());
            for trace in &traces {
                eprintln!("{}", render_plan_evolution_ascii(trace));
            }
        }
    }

    Ok(())
}

fn visualize_ascii(tl: &Timeline, player: &TimelinePlayer, verbose: bool) -> Result<()> {
    // Row count charts per table
    for table_name in &tl.table_names() {
        let series = build_row_count_series(tl, table_name);
        let config = ChartConfig {
            title: format!("{table_name} - Row Count"),
            y_label: "Rows".to_owned(),
            x_label: "Time (s)".to_owned(),
            width: 60,
            height: 15,
        };
        eprintln!("{}", render_ascii_bar_chart(&series, &config));
    }

    // NDV evolution if verbose
    if verbose {
        let ndv_series = build_ndv_series(tl);
        if !ndv_series.is_empty() {
            let config = ChartConfig {
                title: "NDV Evolution".to_owned(),
                y_label: "NDV".to_owned(),
                x_label: "Time (s)".to_owned(),
                ..ChartConfig::default()
            };
            eprintln!("{}", render_ascii_sparkline(&ndv_series, &config));
        }
    }

    // Q-error chart if feedback exists
    if !tl.feedback.is_empty() {
        let q_series = build_q_error_series(tl);
        let config = ChartConfig {
            title: "Q-Error Over Time".to_owned(),
            y_label: "Q-Error".to_owned(),
            x_label: "Time (s)".to_owned(),
            width: 60,
            height: 15,
        };
        eprintln!("{}", render_ascii_bar_chart(&q_series, &config));

        // Estimated vs Actual
        let est = build_estimated_series(tl);
        let act = build_actual_series(tl);
        let config2 = ChartConfig {
            title: "Estimated vs Actual Rows".to_owned(),
            ..ChartConfig::default()
        };
        eprintln!("{}", render_ascii_sparkline(&[est, act], &config2));
    }

    // Plan evolution section
    let traces = build_plan_evolution_traces(tl);
    for trace in &traces {
        eprintln!("{}", render_plan_evolution_ascii(trace));
    }

    // Summary line
    if let Some(avg_q) = player.average_q_error() {
        eprintln!(
            "Average Q-error: {:.2}, Max: {:.2}",
            avg_q,
            player.max_q_error().unwrap_or(0.0),
        );
    }

    Ok(())
}

fn visualize_html(tl: &Timeline, _player: &TimelinePlayer) -> Result<()> {
    let mut all_series = Vec::new();

    for table_name in &tl.table_names() {
        all_series.push(build_row_count_series(tl, table_name));
    }

    if !tl.feedback.is_empty() {
        all_series.push(build_estimated_series(tl));
        all_series.push(build_actual_series(tl));
    }

    let config = ChartConfig {
        title: format!("{} - Statistics Visualization", tl.metadata.name,),
        y_label: "Value".to_owned(),
        x_label: "Time (s)".to_owned(),
        width: 900,
        height: 450,
    };

    println!("{}", render_html_chart(&all_series, &config));
    Ok(())
}

fn visualize_json(tl: &Timeline, player: &TimelinePlayer) -> Result<()> {
    let mut tables_json = serde_json::Map::new();

    for table_name in &tl.table_names() {
        let snapshots: Vec<serde_json::Value> = tl
            .snapshots
            .iter()
            .filter_map(|s| {
                s.tables.iter().find(|t| t.name == *table_name).map(|t| {
                    serde_json::json!({
                        "time_offset": s.time_offset,
                        "row_count": t.row_count,
                        "page_count": t.page_count,
                        "avg_row_size": t.avg_row_size,
                    })
                })
            })
            .collect();
        tables_json.insert(table_name.clone(), serde_json::Value::Array(snapshots));
    }

    let result = serde_json::json!({
        "timeline": tl.metadata.name,
        "description": tl.metadata.description,
        "snapshots": tl.snapshot_count(),
        "events": tl.event_count(),
        "feedback_entries": tl.feedback_count(),
        "time_span": tl.time_span(),
        "average_q_error": player.average_q_error(),
        "max_q_error": player.max_q_error(),
        "tables": tables_json,
    });

    let json =
        serde_json::to_string_pretty(&result).context("serializing visualization to JSON")?;
    println!("{json}");
    Ok(())
}

// -- Plan evolution --

/// Build plan evolution traces from timeline feedback data.
///
/// Groups feedback entries by query, then detects operator changes
/// between consecutive entries and annotates with reasons derived
/// from timeline events.
fn build_plan_evolution_traces(tl: &Timeline) -> Vec<PlanEvolutionTrace> {
    if tl.feedback.is_empty() {
        return Vec::new();
    }

    // Group feedback by query, preserving insertion order.
    let mut keys: Vec<String> = Vec::new();
    let mut groups: std::collections::HashMap<String, Vec<&ExecutionFeedback>> =
        std::collections::HashMap::new();

    for fb in &tl.feedback {
        let key = truncate_query(&fb.query, 60);
        if !groups.contains_key(&key) {
            keys.push(key.clone());
        }
        groups.entry(key).or_default().push(fb);
    }

    let mut traces = Vec::new();

    for query_label in &keys {
        let entries = &groups[query_label];
        let mut snapshots = Vec::new();

        for (i, fb) in entries.iter().enumerate() {
            let operator = fb.operator.as_deref().unwrap_or("unknown").to_owned();
            let cost = fb.estimated_cost;
            let time_label = format!("t={}s", fb.time_offset);

            let (changed, reason) = if i == 0 {
                (false, None)
            } else {
                let prev = entries[i - 1];
                let prev_op = prev.operator.as_deref().unwrap_or("unknown");

                if operators_differ(prev_op, &operator) {
                    let event_strs: Vec<String> = tl
                        .events_in_range(prev.time_offset, fb.time_offset)
                        .iter()
                        .map(|e| format_event_kind(&e.kind))
                        .collect();
                    let event_refs: Vec<&str> = event_strs.iter().map(String::as_str).collect();
                    let reason = infer_change_reason(prev_op, &operator, &event_refs);
                    (true, Some(reason))
                } else {
                    (false, None)
                }
            };

            snapshots.push(PlanSnapshot {
                time_label,
                operator,
                cost,
                changed,
                reason,
            });
        }

        traces.push(PlanEvolutionTrace {
            query_label: query_label.clone(),
            snapshots,
        });
    }

    traces
}

// -- Helpers --

fn print_snapshot_table(snap: &Snapshot) {
    let headers = vec!["Table", "Rows", "Pages", "Avg Size"];
    let rows: Vec<Vec<String>> = snap
        .tables
        .iter()
        .map(|t| {
            vec![
                t.name.clone(),
                format_number(t.row_count),
                t.page_count.map_or("-".to_owned(), format_number),
                t.avg_row_size
                    .map_or("-".to_owned(), |s| format!("{s:.1}B")),
            ]
        })
        .collect();
    eprintln!("{}", render_ascii_table(&headers, &rows));
}

fn print_feedback_entry(fb: &ExecutionFeedback) {
    let direction = if fb.is_overestimate() {
        "OVER".red()
    } else if fb.is_underestimate() {
        "UNDER".yellow()
    } else {
        "EXACT".green()
    };
    eprintln!(
        "    t={}: est={:.0}, act={:.0}, q={:.2} [{}]",
        fb.time_offset,
        fb.estimated_rows,
        fb.actual_rows,
        fb.q_error(),
        direction,
    );
}

fn format_event_kind(kind: &EventKind) -> String {
    match kind {
        EventKind::Insert => "INSERT".to_owned(),
        EventKind::Update => "UPDATE".to_owned(),
        EventKind::Delete => "DELETE".to_owned(),
        EventKind::Analyze => "ANALYZE".to_owned(),
        EventKind::Reoptimize => "REOPTIMIZE".to_owned(),
        EventKind::SchemaChange => "SCHEMA_CHANGE".to_owned(),
        EventKind::Vacuum => "VACUUM".to_owned(),
    }
}

fn format_number(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn truncate_query(query: &str, max_len: usize) -> String {
    if query.len() <= max_len {
        query.to_owned()
    } else {
        format!("{}...", &query[..max_len.saturating_sub(3)])
    }
}

fn build_row_count_series(tl: &Timeline, table: &str) -> Series {
    let points: Vec<DataPoint> = tl
        .snapshots
        .iter()
        .filter_map(|s| {
            s.tables
                .iter()
                .find(|t| t.name == table)
                .map(|t| DataPoint {
                    label: format!("t={}s", s.time_offset),
                    value: t.row_count as f64,
                })
        })
        .collect();

    Series {
        name: format!("{table} rows"),
        points,
    }
}

fn build_q_error_series(tl: &Timeline) -> Series {
    let points: Vec<DataPoint> = tl
        .feedback
        .iter()
        .map(|fb| DataPoint {
            label: format!("t={}s", fb.time_offset),
            value: fb.q_error(),
        })
        .collect();

    Series {
        name: "Q-error".to_owned(),
        points,
    }
}

fn build_estimated_series(tl: &Timeline) -> Series {
    let points: Vec<DataPoint> = tl
        .feedback
        .iter()
        .map(|fb| DataPoint {
            label: format!("t={}s", fb.time_offset),
            value: fb.estimated_rows,
        })
        .collect();

    Series {
        name: "Estimated".to_owned(),
        points,
    }
}

fn build_actual_series(tl: &Timeline) -> Series {
    let points: Vec<DataPoint> = tl
        .feedback
        .iter()
        .map(|fb| DataPoint {
            label: format!("t={}s", fb.time_offset),
            value: fb.actual_rows,
        })
        .collect();

    Series {
        name: "Actual".to_owned(),
        points,
    }
}

fn build_ndv_series(tl: &Timeline) -> Vec<Series> {
    let mut series_map: std::collections::HashMap<String, Vec<DataPoint>> =
        std::collections::HashMap::new();

    for snap in &tl.snapshots {
        for table in &snap.tables {
            for col in &table.columns {
                let key = format!("{}.{}", table.name, col.name);
                series_map.entry(key).or_default().push(DataPoint {
                    label: format!("t={}s", snap.time_offset),
                    value: col.ndv as f64,
                });
            }
        }
    }

    let mut result: Vec<Series> = series_map
        .into_iter()
        .map(|(name, points)| Series { name, points })
        .collect();
    result.sort_by(|a, b| a.name.cmp(&b.name));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_timeline_path() -> String {
        let base = env!("CARGO_MANIFEST_DIR");
        let project_root = base.strip_suffix("/crates/ra-cli").unwrap_or(base);
        format!("{project_root}/timelines/tpch-q1-evolution.toml")
    }

    fn streaming_timeline_path() -> String {
        let base = env!("CARGO_MANIFEST_DIR");
        let project_root = base.strip_suffix("/crates/ra-cli").unwrap_or(base);
        format!("{project_root}/timelines/streaming-inserts.toml")
    }

    fn bulk_update_path() -> String {
        let base = env!("CARGO_MANIFEST_DIR");
        let project_root = base.strip_suffix("/crates/ra-cli").unwrap_or(base);
        format!("{project_root}/timelines/bulk-update-skew.toml")
    }

    fn multi_table_path() -> String {
        let base = env!("CARGO_MANIFEST_DIR");
        let project_root = base.strip_suffix("/crates/ra-cli").unwrap_or(base);
        format!("{project_root}/timelines/multi-table-join.toml")
    }

    fn analyze_loop_path() -> String {
        let base = env!("CARGO_MANIFEST_DIR");
        let project_root = base.strip_suffix("/crates/ra-cli").unwrap_or(base);
        format!("{project_root}/timelines/analyze-feedback-loop.toml")
    }

    fn delete_heavy_path() -> String {
        let base = env!("CARGO_MANIFEST_DIR");
        let project_root = base.strip_suffix("/crates/ra-cli").unwrap_or(base);
        format!("{project_root}/timelines/delete-heavy-workload.toml")
    }

    // -- OutputFormat --

    #[test]
    fn output_format_table() {
        assert_eq!(
            OutputFormat::from_str_arg("table").ok(),
            Some(OutputFormat::Table),
        );
    }

    #[test]
    fn output_format_json() {
        assert_eq!(
            OutputFormat::from_str_arg("json").ok(),
            Some(OutputFormat::Json),
        );
    }

    #[test]
    fn output_format_ascii() {
        assert_eq!(
            OutputFormat::from_str_arg("ascii").ok(),
            Some(OutputFormat::Ascii),
        );
    }

    #[test]
    fn output_format_html() {
        assert_eq!(
            OutputFormat::from_str_arg("html").ok(),
            Some(OutputFormat::Html),
        );
    }

    #[test]
    fn output_format_case_insensitive() {
        assert_eq!(
            OutputFormat::from_str_arg("TABLE").ok(),
            Some(OutputFormat::Table),
        );
        assert_eq!(
            OutputFormat::from_str_arg("Json").ok(),
            Some(OutputFormat::Json),
        );
    }

    #[test]
    fn output_format_invalid() {
        assert!(OutputFormat::from_str_arg("xml").is_err());
    }

    // -- load_timeline --

    #[test]
    fn load_timeline_success() {
        let tl = load_timeline(&test_timeline_path());
        assert!(tl.is_ok());
    }

    #[test]
    fn load_timeline_not_found() {
        let result = load_timeline("/nonexistent/path.toml");
        assert!(result.is_err());
        let msg = format!("{}", result.err().expect("should fail"));
        assert!(msg.contains("not found"));
    }

    // -- format_event_kind --

    #[test]
    fn event_kind_formatting() {
        assert_eq!(format_event_kind(&EventKind::Insert), "INSERT");
        assert_eq!(format_event_kind(&EventKind::Update), "UPDATE");
        assert_eq!(format_event_kind(&EventKind::Delete), "DELETE");
        assert_eq!(format_event_kind(&EventKind::Analyze), "ANALYZE");
        assert_eq!(format_event_kind(&EventKind::Reoptimize), "REOPTIMIZE",);
        assert_eq!(format_event_kind(&EventKind::SchemaChange), "SCHEMA_CHANGE",);
        assert_eq!(format_event_kind(&EventKind::Vacuum), "VACUUM");
    }

    // -- format_number --

    #[test]
    fn format_number_small() {
        assert_eq!(format_number(42), "42");
        assert_eq!(format_number(999), "999");
    }

    #[test]
    fn format_number_thousands() {
        assert_eq!(format_number(1_000), "1.0K");
        assert_eq!(format_number(1_500), "1.5K");
        assert_eq!(format_number(999_999), "1000.0K");
    }

    #[test]
    fn format_number_millions() {
        assert_eq!(format_number(1_000_000), "1.0M");
        assert_eq!(format_number(6_001_215), "6.0M");
    }

    // -- truncate_query --

    #[test]
    fn truncate_query_short() {
        let q = "SELECT * FROM t";
        assert_eq!(truncate_query(q, 30), q);
    }

    #[test]
    fn truncate_query_long() {
        let q = "SELECT very_long_column_name FROM very_long_table";
        let truncated = truncate_query(q, 20);
        assert!(truncated.len() <= 20);
        assert!(truncated.ends_with("..."));
    }

    // -- build series helpers --

    #[test]
    fn build_row_count_series_tpch() {
        let tl = load_timeline(&test_timeline_path()).expect("load");
        let series = build_row_count_series(&tl, "lineitem");
        assert!(!series.points.is_empty());
        assert_eq!(series.name, "lineitem rows");
    }

    #[test]
    fn build_row_count_series_nonexistent_table() {
        let tl = load_timeline(&test_timeline_path()).expect("load");
        let series = build_row_count_series(&tl, "nonexistent");
        assert!(series.points.is_empty());
    }

    #[test]
    fn build_q_error_series_with_feedback() {
        let tl = load_timeline(&test_timeline_path()).expect("load");
        let series = build_q_error_series(&tl);
        assert!(!series.points.is_empty());
        for point in &series.points {
            assert!(point.value >= 1.0);
        }
    }

    #[test]
    fn build_estimated_series_with_feedback() {
        let tl = load_timeline(&test_timeline_path()).expect("load");
        let series = build_estimated_series(&tl);
        assert!(!series.points.is_empty());
        assert_eq!(series.name, "Estimated");
    }

    #[test]
    fn build_actual_series_with_feedback() {
        let tl = load_timeline(&test_timeline_path()).expect("load");
        let series = build_actual_series(&tl);
        assert!(!series.points.is_empty());
        assert_eq!(series.name, "Actual");
    }

    #[test]
    fn build_ndv_series_tpch() {
        let tl = load_timeline(&test_timeline_path()).expect("load");
        let series = build_ndv_series(&tl);
        assert!(!series.is_empty());
    }

    // -- Integration: play command --

    #[test]
    fn play_table_tpch() {
        let result = cmd_stats_play(&test_timeline_path(), OutputFormat::Table, 0.0, false);
        assert!(result.is_ok());
    }

    #[test]
    fn play_json_tpch() {
        let result = cmd_stats_play(&test_timeline_path(), OutputFormat::Json, 0.0, false);
        assert!(result.is_ok());
    }

    #[test]
    fn play_ascii_tpch() {
        let result = cmd_stats_play(&test_timeline_path(), OutputFormat::Ascii, 0.0, false);
        assert!(result.is_ok());
    }

    #[test]
    fn play_html_tpch() {
        let result = cmd_stats_play(&test_timeline_path(), OutputFormat::Html, 0.0, false);
        assert!(result.is_ok());
    }

    #[test]
    fn play_table_streaming() {
        let result = cmd_stats_play(&streaming_timeline_path(), OutputFormat::Table, 0.0, true);
        assert!(result.is_ok());
    }

    #[test]
    fn play_table_bulk_update() {
        let result = cmd_stats_play(&bulk_update_path(), OutputFormat::Table, 0.0, false);
        assert!(result.is_ok());
    }

    #[test]
    fn play_nonexistent_file() {
        let result = cmd_stats_play("/nonexistent.toml", OutputFormat::Table, 1.0, false);
        assert!(result.is_err());
    }

    // -- Integration: feedback command --

    #[test]
    fn feedback_table_tpch() {
        let result = cmd_stats_feedback(&test_timeline_path(), OutputFormat::Table, 2, false);
        assert!(result.is_ok());
    }

    #[test]
    fn feedback_json_tpch() {
        let result = cmd_stats_feedback(&test_timeline_path(), OutputFormat::Json, 5, false);
        assert!(result.is_ok());
    }

    #[test]
    fn feedback_ascii_tpch() {
        let result = cmd_stats_feedback(&test_timeline_path(), OutputFormat::Ascii, 3, false);
        assert!(result.is_ok());
    }

    #[test]
    fn feedback_html_tpch() {
        let result = cmd_stats_feedback(&test_timeline_path(), OutputFormat::Html, 3, false);
        assert!(result.is_ok());
    }

    #[test]
    fn feedback_verbose_tpch() {
        let result = cmd_stats_feedback(&test_timeline_path(), OutputFormat::Table, 2, true);
        assert!(result.is_ok());
    }

    #[test]
    fn feedback_batch_size_one() {
        let result = cmd_stats_feedback(&test_timeline_path(), OutputFormat::Table, 1, false);
        assert!(result.is_ok());
    }

    #[test]
    fn feedback_large_batch() {
        let result = cmd_stats_feedback(&test_timeline_path(), OutputFormat::Table, 100, false);
        assert!(result.is_ok());
    }

    #[test]
    fn feedback_streaming_inserts() {
        let result = cmd_stats_feedback(&streaming_timeline_path(), OutputFormat::Table, 3, false);
        assert!(result.is_ok());
    }

    #[test]
    fn feedback_analyze_loop() {
        let result = cmd_stats_feedback(&analyze_loop_path(), OutputFormat::Table, 2, true);
        assert!(result.is_ok());
    }

    // -- Integration: visualize command --

    #[test]
    fn visualize_table_tpch() {
        let result = cmd_stats_visualize(&test_timeline_path(), OutputFormat::Table, false);
        assert!(result.is_ok());
    }

    #[test]
    fn visualize_ascii_tpch() {
        let result = cmd_stats_visualize(&test_timeline_path(), OutputFormat::Ascii, false);
        assert!(result.is_ok());
    }

    #[test]
    fn visualize_html_tpch() {
        let result = cmd_stats_visualize(&test_timeline_path(), OutputFormat::Html, false);
        assert!(result.is_ok());
    }

    #[test]
    fn visualize_json_tpch() {
        let result = cmd_stats_visualize(&test_timeline_path(), OutputFormat::Json, false);
        assert!(result.is_ok());
    }

    #[test]
    fn visualize_verbose_tpch() {
        let result = cmd_stats_visualize(&test_timeline_path(), OutputFormat::Table, true);
        assert!(result.is_ok());
    }

    #[test]
    fn visualize_ascii_verbose_tpch() {
        let result = cmd_stats_visualize(&test_timeline_path(), OutputFormat::Ascii, true);
        assert!(result.is_ok());
    }

    // -- All example timelines --

    #[test]
    fn play_all_examples_table() {
        let paths = [
            test_timeline_path(),
            streaming_timeline_path(),
            bulk_update_path(),
            multi_table_path(),
            analyze_loop_path(),
            delete_heavy_path(),
        ];
        for path in &paths {
            let result = cmd_stats_play(path, OutputFormat::Table, 0.0, false);
            assert!(
                result.is_ok(),
                "play table failed for {path}: {}",
                result.err().map_or(String::new(), |e| e.to_string()),
            );
        }
    }

    #[test]
    fn visualize_all_examples_ascii() {
        let paths = [
            test_timeline_path(),
            streaming_timeline_path(),
            bulk_update_path(),
            multi_table_path(),
            analyze_loop_path(),
            delete_heavy_path(),
        ];
        for path in &paths {
            let result = cmd_stats_visualize(path, OutputFormat::Ascii, false);
            assert!(
                result.is_ok(),
                "visualize ascii failed for {path}: {}",
                result.err().map_or(String::new(), |e| e.to_string()),
            );
        }
    }

    #[test]
    fn feedback_all_examples_table() {
        let paths = [
            test_timeline_path(),
            streaming_timeline_path(),
            bulk_update_path(),
            multi_table_path(),
            analyze_loop_path(),
            delete_heavy_path(),
        ];
        for path in &paths {
            let result = cmd_stats_feedback(path, OutputFormat::Table, 3, false);
            assert!(
                result.is_ok(),
                "feedback table failed for {path}: {}",
                result.err().map_or(String::new(), |e| e.to_string()),
            );
        }
    }

    #[test]
    fn visualize_all_examples_json() {
        let paths = [
            test_timeline_path(),
            streaming_timeline_path(),
            bulk_update_path(),
            multi_table_path(),
            analyze_loop_path(),
            delete_heavy_path(),
        ];
        for path in &paths {
            let result = cmd_stats_visualize(path, OutputFormat::Json, false);
            assert!(
                result.is_ok(),
                "visualize json failed for {path}: {}",
                result.err().map_or(String::new(), |e| e.to_string()),
            );
        }
    }

    // -- Plan evolution integration tests --

    #[test]
    fn build_traces_tpch_q1() {
        let tl = load_timeline(&test_timeline_path()).expect("load");
        let traces = build_plan_evolution_traces(&tl);
        assert!(
            !traces.is_empty(),
            "tpch-q1 has feedback, should produce traces"
        );
        // All feedback has the same operator, so no changes
        let trace = &traces[0];
        assert!(!trace.snapshots.is_empty());
        assert!(!trace.snapshots[0].changed);
    }

    #[test]
    fn build_traces_renders_without_panic() {
        let tl = load_timeline(&test_timeline_path()).expect("load");
        let traces = build_plan_evolution_traces(&tl);
        for trace in &traces {
            let out = render_plan_evolution_ascii(trace);
            assert!(out.contains("Plan Evolution:"));
        }
    }

    #[test]
    fn build_traces_streaming_inserts() {
        let tl = load_timeline(&streaming_timeline_path()).expect("load");
        let traces = build_plan_evolution_traces(&tl);
        // Streaming inserts has feedback entries
        if !tl.feedback.is_empty() {
            assert!(!traces.is_empty());
        }
    }

    #[test]
    fn build_traces_analyze_loop() {
        let tl = load_timeline(&analyze_loop_path()).expect("load");
        let traces = build_plan_evolution_traces(&tl);
        if !tl.feedback.is_empty() {
            assert!(!traces.is_empty());
        }
    }

    #[test]
    fn build_traces_empty_feedback() {
        let tl = Timeline::from_toml(
            r#"
[metadata]
name = "empty-fb"
description = "no feedback"

[[snapshots]]
time_offset = 0

[[snapshots.tables]]
name = "t"
row_count = 100
"#,
        )
        .expect("parse");
        let traces = build_plan_evolution_traces(&tl);
        assert!(traces.is_empty());
    }

    #[test]
    fn build_traces_operator_change_detected() {
        let tl = Timeline::from_toml(
            r#"
[metadata]
name = "plan-change"
description = "Operator changes between feedback entries"

[[snapshots]]
time_offset = 0

[[snapshots.tables]]
name = "t"
row_count = 100

[[snapshots]]
time_offset = 3600

[[snapshots.tables]]
name = "t"
row_count = 200

[[events]]
time_offset = 1800
kind = "analyze"
table = "t"

[[feedback]]
time_offset = 100
query = "SELECT * FROM t"
operator = "SeqScan on t"
estimated_rows = 100.0
actual_rows = 100.0

[[feedback]]
time_offset = 3600
query = "SELECT * FROM t"
operator = "IndexScan on t"
estimated_rows = 200.0
actual_rows = 200.0
"#,
        )
        .expect("parse");
        let traces = build_plan_evolution_traces(&tl);
        assert_eq!(traces.len(), 1);
        let trace = &traces[0];
        assert_eq!(trace.snapshots.len(), 2);
        assert!(!trace.snapshots[0].changed);
        assert!(trace.snapshots[1].changed);
        let reason = trace.snapshots[1].reason.as_deref().unwrap_or("");
        assert!(
            reason.contains("ANALYZE"),
            "should detect ANALYZE event: {reason}"
        );
    }

    #[test]
    fn build_traces_cost_tracked() {
        let tl = Timeline::from_toml(
            r#"
[metadata]
name = "cost-tracking"
description = "Track cost changes"

[[snapshots]]
time_offset = 0

[[snapshots.tables]]
name = "t"
row_count = 100

[[feedback]]
time_offset = 100
query = "SELECT * FROM t"
operator = "SeqScan on t"
estimated_rows = 100.0
actual_rows = 100.0
estimated_cost = 1500.0

[[feedback]]
time_offset = 200
query = "SELECT * FROM t"
operator = "SeqScan on t"
estimated_rows = 110.0
actual_rows = 110.0
estimated_cost = 1600.0
"#,
        )
        .expect("parse");
        let traces = build_plan_evolution_traces(&tl);
        assert_eq!(traces.len(), 1);
        let snaps = &traces[0].snapshots;
        assert!((snaps[0].cost.unwrap_or(0.0) - 1500.0).abs() < f64::EPSILON);
        assert!((snaps[1].cost.unwrap_or(0.0) - 1600.0).abs() < f64::EPSILON);
    }

    #[test]
    fn build_traces_multiple_queries() {
        let tl = Timeline::from_toml(
            r#"
[metadata]
name = "multi-query"
description = "Multiple queries in feedback"

[[snapshots]]
time_offset = 0

[[snapshots.tables]]
name = "t"
row_count = 100

[[feedback]]
time_offset = 100
query = "SELECT * FROM t WHERE id = 1"
operator = "IndexScan"
estimated_rows = 1.0
actual_rows = 1.0

[[feedback]]
time_offset = 200
query = "SELECT COUNT(*) FROM t"
operator = "SeqScan"
estimated_rows = 100.0
actual_rows = 100.0

[[feedback]]
time_offset = 300
query = "SELECT * FROM t WHERE id = 1"
operator = "IndexScan"
estimated_rows = 1.0
actual_rows = 1.0
"#,
        )
        .expect("parse");
        let traces = build_plan_evolution_traces(&tl);
        assert_eq!(traces.len(), 2, "should group by distinct query");
    }

    #[test]
    fn visualize_ascii_with_plan_evolution() {
        // Verify visualize_ascii runs with plan evolution
        // without panicking on all example timelines
        let paths = [
            test_timeline_path(),
            streaming_timeline_path(),
            bulk_update_path(),
            multi_table_path(),
            analyze_loop_path(),
            delete_heavy_path(),
        ];
        for path in &paths {
            let result = cmd_stats_visualize(path, OutputFormat::Ascii, false);
            assert!(
                result.is_ok(),
                "visualize ascii with evolution failed for {path}"
            );
        }
    }

    #[test]
    fn visualize_table_with_plan_evolution() {
        let result = cmd_stats_visualize(&test_timeline_path(), OutputFormat::Table, true);
        assert!(result.is_ok());
    }
}
