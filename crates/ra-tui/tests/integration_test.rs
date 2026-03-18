//! Integration tests for the ra-tui crate.
//!
//! Tests the TUI components working together: app state machine,
//! playback, panels, layout, and headless mode.

#![allow(clippy::expect_used)]

use ra_tui::app::{App, AppError, Panel};
use ra_tui::event::Action;
use ra_tui::layout;
use ra_tui::panels::{evolution, feedback, plan_tree, statistics};
use ra_tui::playback::PlaybackController;
use ra_tui::setup::{SetupError, TuiConfig};
use ra_tui::timeline::{Snapshot, TableStatEntry, Timeline};
use ra_tui::ui;

use ratatui::layout::Rect;

// -- Test helpers --

fn sample_timeline() -> Timeline {
    Timeline::demo()
}

fn minimal_timeline() -> Timeline {
    let mut tl = Timeline::new("SELECT 1", "auto");
    tl.push(Snapshot {
        label: "step 0".into(),
        step: 0,
        plan_text: "Scan(t)\n".into(),
        cost: 100.0,
        rules_applied: vec!["rule-a".into()],
        table_stats: vec![TableStatEntry {
            table: "t".into(),
            row_count: 1000,
            staleness: "Fresh".into(),
            confidence: 0.95,
        }],
        diagnostics: vec!["Initial parse".into()],
    });
    tl.push(Snapshot {
        label: "step 1".into(),
        step: 1,
        plan_text: "Index Scan(t, idx_pk)\n".into(),
        cost: 50.0,
        rules_applied: vec!["index-select".into()],
        table_stats: vec![TableStatEntry {
            table: "t".into(),
            row_count: 1000,
            staleness: "Fresh".into(),
            confidence: 0.95,
        }],
        diagnostics: vec!["Used index".into()],
    });
    tl
}

fn area(w: u16, h: u16) -> Rect {
    Rect::new(0, 0, w, h)
}

// -- App creation --

#[test]
fn create_app_from_demo_timeline() {
    let tl = sample_timeline();
    let app = App::new(tl).expect("app creation");
    assert_eq!(app.current_step, 0);
    assert!(!app.playing);
    assert!(!app.should_quit);
}

#[test]
fn create_app_from_minimal_timeline() {
    let tl = minimal_timeline();
    let app = App::new(tl).expect("app creation");
    assert_eq!(app.timeline.len(), 2);
}

#[test]
fn create_app_empty_timeline_fails() {
    let tl = Timeline::new("SELECT 1", "auto");
    let result = App::new(tl);
    assert!(result.is_err());
}

#[test]
fn app_error_display() {
    let err = AppError::EmptyTimeline;
    let msg = format!("{err}");
    assert!(msg.contains("no snapshots"));
}

// -- App action handling --

#[test]
fn app_quit_action() {
    let mut app =
        App::new(sample_timeline()).expect("app creation");
    app.handle_action(Action::Quit);
    assert!(app.should_quit);
}

#[test]
fn app_next_step() {
    let mut app =
        App::new(sample_timeline()).expect("app creation");
    app.handle_action(Action::NextStep);
    assert_eq!(app.current_step, 1);
}

#[test]
fn app_prev_step_at_zero() {
    let mut app =
        App::new(sample_timeline()).expect("app creation");
    app.handle_action(Action::PrevStep);
    assert_eq!(app.current_step, 0);
}

#[test]
fn app_next_then_prev() {
    let mut app =
        App::new(sample_timeline()).expect("app creation");
    app.handle_action(Action::NextStep);
    app.handle_action(Action::NextStep);
    app.handle_action(Action::PrevStep);
    assert_eq!(app.current_step, 1);
}

#[test]
fn app_first_step() {
    let mut app =
        App::new(sample_timeline()).expect("app creation");
    app.handle_action(Action::NextStep);
    app.handle_action(Action::NextStep);
    app.handle_action(Action::FirstStep);
    assert_eq!(app.current_step, 0);
}

#[test]
fn app_last_step() {
    let mut app =
        App::new(sample_timeline()).expect("app creation");
    app.handle_action(Action::LastStep);
    assert_eq!(
        app.current_step,
        app.timeline.len() - 1
    );
}

#[test]
fn app_toggle_play() {
    let mut app =
        App::new(sample_timeline()).expect("app creation");
    app.handle_action(Action::TogglePlay);
    assert!(app.playing);
    app.handle_action(Action::TogglePlay);
    assert!(!app.playing);
}

#[test]
fn app_speed_up() {
    let mut app =
        App::new(sample_timeline()).expect("app creation");
    let initial = app.speed_label().to_owned();
    app.handle_action(Action::SpeedUp);
    assert_ne!(app.speed_label(), initial);
}

#[test]
fn app_slow_down() {
    let mut app =
        App::new(sample_timeline()).expect("app creation");
    app.handle_action(Action::SpeedUp);
    app.handle_action(Action::SpeedUp);
    app.handle_action(Action::SlowDown);
    assert_eq!(app.speed_label(), "2x");
}

#[test]
fn app_next_panel() {
    let mut app =
        App::new(sample_timeline()).expect("app creation");
    assert_eq!(app.focused, Panel::Plan);
    app.handle_action(Action::NextPanel);
    assert_eq!(app.focused, Panel::Evolution);
}

#[test]
fn app_prev_panel() {
    let mut app =
        App::new(sample_timeline()).expect("app creation");
    app.handle_action(Action::PrevPanel);
    assert_eq!(app.focused, Panel::Stats);
}

#[test]
fn app_panel_cycle() {
    let mut app =
        App::new(sample_timeline()).expect("app creation");
    for _ in 0..4 {
        app.handle_action(Action::NextPanel);
    }
    // Should cycle back to Plan
    assert_eq!(app.focused, Panel::Plan);
}

#[test]
fn app_scroll_down_up() {
    let mut app =
        App::new(sample_timeline()).expect("app creation");
    app.handle_action(Action::ScrollDown);
    assert_eq!(app.scroll_offset, 1);
    app.handle_action(Action::ScrollDown);
    assert_eq!(app.scroll_offset, 2);
    app.handle_action(Action::ScrollUp);
    assert_eq!(app.scroll_offset, 1);
}

#[test]
fn app_scroll_up_at_zero() {
    let mut app =
        App::new(sample_timeline()).expect("app creation");
    app.handle_action(Action::ScrollUp);
    assert_eq!(app.scroll_offset, 0);
}

#[test]
fn app_toggle_help() {
    let mut app =
        App::new(sample_timeline()).expect("app creation");
    assert!(!app.show_help);
    app.handle_action(Action::ToggleHelp);
    assert!(app.show_help);
    app.handle_action(Action::ToggleHelp);
    assert!(!app.show_help);
}

#[test]
fn app_none_action() {
    let mut app =
        App::new(sample_timeline()).expect("app creation");
    let step_before = app.current_step;
    app.handle_action(Action::None);
    assert_eq!(app.current_step, step_before);
}

#[test]
fn app_panel_change_resets_scroll() {
    let mut app =
        App::new(sample_timeline()).expect("app creation");
    app.handle_action(Action::ScrollDown);
    app.handle_action(Action::ScrollDown);
    app.handle_action(Action::NextPanel);
    assert_eq!(app.scroll_offset, 0);
}

// -- Headless mode --

#[test]
fn headless_run_demo() {
    let tl = sample_timeline();
    let mut app = App::new(tl).expect("app creation");
    let final_cost =
        app.run_headless().expect("headless run");
    assert!(final_cost > 0.0);
}

#[test]
fn headless_run_minimal() {
    let tl = minimal_timeline();
    let mut app = App::new(tl).expect("app creation");
    let final_cost =
        app.run_headless().expect("headless run");
    assert!((final_cost - 50.0).abs() < f64::EPSILON);
}

#[test]
fn headless_advances_to_last_step() {
    let tl = sample_timeline();
    let expected_last = tl.len() - 1;
    let mut app = App::new(tl).expect("app creation");
    app.run_headless().expect("headless run");
    assert_eq!(app.current_step, expected_last);
}

// -- PlaybackController integration --

#[test]
fn controller_with_demo_timeline() {
    let tl = sample_timeline();
    let ctrl = PlaybackController::new(tl.len());
    assert_eq!(ctrl.total_steps(), tl.len());
    assert!(ctrl.has_next());
}

#[test]
fn controller_full_walkthrough() {
    let tl = sample_timeline();
    let mut ctrl = PlaybackController::new(tl.len());
    let mut steps = 0;
    while ctrl.step_forward() {
        steps += 1;
    }
    assert_eq!(steps, tl.len() - 1);
    assert!(!ctrl.has_next());
}

#[test]
fn controller_speed_labels_are_valid() {
    let mut ctrl = PlaybackController::new(5);
    let mut labels = Vec::new();
    labels.push(ctrl.speed_label().to_owned());
    for _ in 0..10 {
        ctrl.speed_up();
        labels.push(ctrl.speed_label().to_owned());
    }
    for label in &labels {
        assert!(label.ends_with('x'));
    }
}

// -- Layout --

#[test]
fn layout_frame_standard_terminal() {
    let fl = layout::frame_layout(area(120, 40));
    assert!(fl.content.width > 0);
    assert!(fl.content.height > 0);
}

#[test]
fn layout_panels_standard_terminal() {
    let pl = layout::panel_layout(area(120, 40));
    assert!(pl.stats.width > 0);
    assert!(pl.plan.width > 0);
    assert!(pl.evolution.width > 0);
    assert!(pl.feedback.width > 0);
}

#[test]
fn layout_panels_all_have_area() {
    let pl = layout::panel_layout(area(120, 40));
    assert!(pl.stats.area() > 0);
    assert!(pl.plan.area() > 0);
    assert!(pl.evolution.area() > 0);
    assert!(pl.feedback.area() > 0);
}

// -- Panel rendering (data preparation) --

#[test]
fn evolution_chart_from_demo_data() {
    let tl = sample_timeline();
    let costs: Vec<f64> =
        tl.snapshots.iter().map(|s| s.cost).collect();
    let lines =
        evolution::build_chart_lines(&costs, 0, area(80, 20));
    assert!(lines.len() > 2);
}

#[test]
fn feedback_lines_from_demo_snapshot() {
    let tl = sample_timeline();
    let snap = &tl.snapshots[1]; // After predicate pushdown
    let lines = feedback::build_feedback_lines(snap);
    assert!(!lines.is_empty());
}

#[test]
fn plan_tree_colors_demo_snapshot() {
    let tl = sample_timeline();
    for snap in &tl.snapshots {
        for line in snap.plan_text.lines() {
            // Should not panic
            let _color = plan_tree::plan_node_color(line);
        }
    }
}

#[test]
fn statistics_format_all_demo_counts() {
    let tl = sample_timeline();
    for snap in &tl.snapshots {
        for stat in &snap.table_stats {
            let formatted =
                statistics::format_row_count(stat.row_count);
            assert!(!formatted.is_empty());
        }
    }
}

// -- UI helpers --

#[test]
fn truncate_str_demo_query() {
    let tl = sample_timeline();
    let truncated = ui::truncate_str(&tl.query, 30);
    assert!(truncated.len() <= 30);
}

#[test]
fn cost_color_demo_costs() {
    let tl = sample_timeline();
    let app = App::new(tl).expect("app creation");
    for snap in &app.timeline.snapshots {
        // Should not panic
        let _color = ui::cost_color(snap.cost, &app);
    }
}

// -- Timeline --

#[test]
fn timeline_demo_has_snapshots() {
    let tl = Timeline::demo();
    assert!(tl.len() >= 5);
}

#[test]
fn timeline_demo_costs_decrease() {
    let tl = Timeline::demo();
    assert!(
        tl.snapshots.last().map_or(0.0, |s| s.cost)
            < tl.snapshots.first().map_or(0.0, |s| s.cost)
    );
}

#[test]
fn timeline_demo_has_table_stats() {
    let tl = Timeline::demo();
    for snap in &tl.snapshots {
        assert!(!snap.table_stats.is_empty());
    }
}

#[test]
fn timeline_demo_has_plan_text() {
    let tl = Timeline::demo();
    for snap in &tl.snapshots {
        assert!(!snap.plan_text.is_empty());
    }
}

#[test]
fn timeline_new_is_empty() {
    let tl = Timeline::new("SELECT 1", "auto");
    assert!(tl.is_empty());
    assert_eq!(tl.len(), 0);
}

#[test]
fn timeline_push_increments_len() {
    let mut tl = Timeline::new("SELECT 1", "auto");
    assert_eq!(tl.len(), 0);
    tl.push(Snapshot {
        label: "s".into(),
        step: 0,
        plan_text: "Scan".into(),
        cost: 10.0,
        rules_applied: vec![],
        table_stats: vec![],
        diagnostics: vec![],
    });
    assert_eq!(tl.len(), 1);
    assert!(!tl.is_empty());
}

#[test]
fn timeline_serialization_roundtrip() {
    let tl = minimal_timeline();
    let json = serde_json::to_string(&tl)
        .expect("serialize");
    let tl2: Timeline =
        serde_json::from_str(&json).expect("deserialize");
    assert_eq!(tl2.len(), tl.len());
    assert_eq!(tl2.query, tl.query);
}

// -- Setup --

#[test]
fn tui_config_from_timeline() {
    let tl = minimal_timeline();
    let config = TuiConfig::from_timeline(tl);
    assert!(!config.headless);
    assert_eq!(config.initial_speed, 2);
}

#[test]
fn tui_config_headless_mode() {
    let tl = minimal_timeline();
    let config = TuiConfig::headless(tl);
    assert!(config.headless);
}

#[test]
fn setup_error_variants() {
    let err1 = SetupError::IoError("io".into());
    let err2 = SetupError::ParseError("parse".into());
    let err3 = SetupError::NoTimeline("none".into());
    assert!(format!("{err1}").contains("io"));
    assert!(format!("{err2}").contains("parse"));
    assert!(format!("{err3}").contains("none"));
}

// -- Event action mapping --

#[test]
fn action_equality() {
    assert_eq!(Action::Quit, Action::Quit);
    assert_ne!(Action::Quit, Action::None);
    assert_eq!(Action::NextStep, Action::NextStep);
}

// -- Edge cases --

#[test]
fn single_snapshot_app() {
    let mut tl = Timeline::new("SELECT 1", "auto");
    tl.push(Snapshot {
        label: "only".into(),
        step: 0,
        plan_text: "Scan(t)".into(),
        cost: 42.0,
        rules_applied: vec![],
        table_stats: vec![],
        diagnostics: vec![],
    });
    let mut app = App::new(tl).expect("app creation");
    // Forward should not advance past the only step
    app.handle_action(Action::NextStep);
    assert_eq!(app.current_step, 0);
    // Backward from start stays
    app.handle_action(Action::PrevStep);
    assert_eq!(app.current_step, 0);
}

#[test]
fn single_snapshot_headless() {
    let mut tl = Timeline::new("SELECT 1", "auto");
    tl.push(Snapshot {
        label: "only".into(),
        step: 0,
        plan_text: "Scan".into(),
        cost: 42.0,
        rules_applied: vec![],
        table_stats: vec![],
        diagnostics: vec![],
    });
    let mut app = App::new(tl).expect("app creation");
    let cost =
        app.run_headless().expect("headless");
    assert!((cost - 42.0).abs() < f64::EPSILON);
}

#[test]
fn many_rapid_actions() {
    let mut app =
        App::new(sample_timeline()).expect("app creation");
    for _ in 0..100 {
        app.handle_action(Action::NextStep);
        app.handle_action(Action::PrevStep);
        app.handle_action(Action::ScrollDown);
        app.handle_action(Action::ScrollUp);
    }
    // Should not panic and should be at step 0
    assert_eq!(app.current_step, 0);
}

#[test]
fn all_panels_reachable() {
    let mut app =
        App::new(sample_timeline()).expect("app creation");
    let mut panels_seen = Vec::new();
    panels_seen.push(app.focused);
    for _ in 0..4 {
        app.handle_action(Action::NextPanel);
        panels_seen.push(app.focused);
    }
    assert!(panels_seen.contains(&Panel::Stats));
    assert!(panels_seen.contains(&Panel::Plan));
    assert!(panels_seen.contains(&Panel::Evolution));
    assert!(panels_seen.contains(&Panel::Feedback));
}

#[test]
fn evolution_cost_reduction_matches_data() {
    let tl = sample_timeline();
    let costs: Vec<f64> =
        tl.snapshots.iter().map(|s| s.cost).collect();
    let max = costs
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let min = costs
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min);
    let reduction = evolution::cost_reduction(max, min);
    assert!(reduction > 0.0);
    assert!(reduction <= 100.0);
}

#[test]
fn snapshot_labels_populated() {
    let tl = sample_timeline();
    for snap in &tl.snapshots {
        assert!(!snap.label.is_empty());
    }
}

#[test]
fn snapshot_step_indices_sequential() {
    let tl = sample_timeline();
    for (i, snap) in tl.snapshots.iter().enumerate() {
        assert_eq!(snap.step, i);
    }
}
