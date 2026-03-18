//! ASCII and HTML chart generation for statistics timeline visualization.
//!
//! Generates terminal-friendly ASCII charts and HTML output with SVG
//! for displaying cost/cardinality evolution over time.

#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::uninlined_format_args)]

use std::fmt::Write as FmtWrite;

/// A single data point on a chart.
#[derive(Debug, Clone)]
pub struct DataPoint {
    /// X-axis label (typically a time offset or snapshot label).
    pub label: String,
    /// Y-axis value.
    pub value: f64,
}

/// A named data series for chart rendering.
#[derive(Debug, Clone)]
pub struct Series {
    /// Series name (shown in legend).
    pub name: String,
    /// Data points.
    pub points: Vec<DataPoint>,
}

/// Chart configuration.
#[derive(Debug, Clone)]
pub struct ChartConfig {
    /// Chart title.
    pub title: String,
    /// Y-axis label.
    pub y_label: String,
    /// X-axis label.
    pub x_label: String,
    /// Chart width in characters (ASCII) or pixels (HTML).
    pub width: usize,
    /// Chart height in characters (ASCII) or pixels (HTML).
    pub height: usize,
}

impl Default for ChartConfig {
    fn default() -> Self {
        Self {
            title: String::new(),
            y_label: "Value".to_owned(),
            x_label: "Time".to_owned(),
            width: 60,
            height: 20,
        }
    }
}

/// Render a single series as an ASCII bar chart.
pub fn render_ascii_bar_chart(
    series: &Series,
    config: &ChartConfig,
) -> String {
    if series.points.is_empty() {
        return "(no data)\n".to_owned();
    }

    let max_val = series
        .points
        .iter()
        .map(|p| p.value)
        .fold(f64::NEG_INFINITY, f64::max);
    let min_val = series
        .points
        .iter()
        .map(|p| p.value)
        .fold(f64::INFINITY, f64::min);

    let max_label_len = series
        .points
        .iter()
        .map(|p| p.label.len())
        .max()
        .unwrap_or(1)
        .max(1);

    let bar_width = config.width.saturating_sub(max_label_len + 15);
    let range = if (max_val - min_val).abs() < f64::EPSILON {
        1.0
    } else {
        max_val - min_val
    };

    let mut out = String::new();

    if !config.title.is_empty() {
        let _ = writeln!(out, "{}", config.title);
        let _ = writeln!(out, "{}", "-".repeat(config.title.len()));
    }

    for point in &series.points {
        let normalized = if range > 0.0 {
            (point.value - min_val) / range
        } else {
            1.0
        };
        let filled = (normalized * bar_width as f64).round() as usize;
        let bar: String = "#".repeat(filled);
        let _ = writeln!(
            out,
            "{:>width$} | {:<bar_w$} {:.1}",
            point.label,
            bar,
            point.value,
            width = max_label_len,
            bar_w = bar_width,
        );
    }

    out
}

/// Render multiple series as an ASCII sparkline chart.
pub fn render_ascii_sparkline(
    series: &[Series],
    config: &ChartConfig,
) -> String {
    if series.is_empty() {
        return "(no data)\n".to_owned();
    }

    let height = config.height.min(30);
    let blocks = [' ', '\u{2581}', '\u{2582}', '\u{2583}',
                  '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}',
                  '\u{2588}'];

    let mut out = String::new();
    if !config.title.is_empty() {
        let _ = writeln!(out, "{}", config.title);
        let _ = writeln!(out, "{}", "-".repeat(config.title.len()));
    }

    for s in series {
        if s.points.is_empty() {
            continue;
        }

        let max_val = s
            .points
            .iter()
            .map(|p| p.value)
            .fold(f64::NEG_INFINITY, f64::max);
        let min_val = s
            .points
            .iter()
            .map(|p| p.value)
            .fold(f64::INFINITY, f64::min);
        let range = if (max_val - min_val).abs() < f64::EPSILON {
            1.0
        } else {
            max_val - min_val
        };

        let _ = write!(out, "  {}: ", s.name);
        for point in &s.points {
            let normalized = (point.value - min_val) / range;
            let idx = (normalized * (blocks.len() - 1) as f64)
                .round() as usize;
            let idx = idx.min(blocks.len() - 1);
            out.push(blocks[idx]);
        }
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "    range: [{:.1}, {:.1}]",
            min_val, max_val,
        );
    }

    // X-axis labels (first and last)
    if let Some(first_series) = series.first() {
        if let (Some(first), Some(last)) = (
            first_series.points.first(),
            first_series.points.last(),
        ) {
            let _ = writeln!(
                out,
                "  {}: {} .. {}",
                config.x_label, first.label, last.label,
            );
        }
    }

    let _ = writeln!(out, "  height: {height} (normalized)");

    out
}

/// Render an ASCII table for timeline data.
pub fn render_ascii_table(
    headers: &[&str],
    rows: &[Vec<String>],
) -> String {
    if headers.is_empty() {
        return String::new();
    }

    let mut col_widths: Vec<usize> = headers
        .iter()
        .map(|h| h.len())
        .collect();

    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < col_widths.len() {
                col_widths[i] = col_widths[i].max(cell.len());
            }
        }
    }

    let mut out = String::new();

    // Header
    let _ = write!(out, "  ");
    for (i, header) in headers.iter().enumerate() {
        if i > 0 {
            let _ = write!(out, "  ");
        }
        let _ = write!(out, "{:<width$}", header, width = col_widths[i]);
    }
    let _ = writeln!(out);

    // Separator
    let _ = write!(out, "  ");
    for (i, w) in col_widths.iter().enumerate() {
        if i > 0 {
            let _ = write!(out, "  ");
        }
        let _ = write!(out, "{}", "-".repeat(*w));
    }
    let _ = writeln!(out);

    // Rows
    for row in rows {
        let _ = write!(out, "  ");
        for (i, cell) in row.iter().enumerate() {
            if i > 0 {
                let _ = write!(out, "  ");
            }
            let w = col_widths.get(i).copied().unwrap_or(cell.len());
            let _ = write!(out, "{:<width$}", cell, width = w);
        }
        let _ = writeln!(out);
    }

    out
}

/// Render an HTML page with SVG line chart.
pub fn render_html_chart(
    series: &[Series],
    config: &ChartConfig,
) -> String {
    let svg_width = config.width.max(400);
    let svg_height = config.height.max(200);
    let margin = 60;
    let plot_w = svg_width - 2 * margin;
    let plot_h = svg_height - 2 * margin;

    let global_max = series
        .iter()
        .flat_map(|s| s.points.iter().map(|p| p.value))
        .fold(f64::NEG_INFINITY, f64::max);
    let global_min = series
        .iter()
        .flat_map(|s| s.points.iter().map(|p| p.value))
        .fold(f64::INFINITY, f64::min);
    let range = if (global_max - global_min).abs() < f64::EPSILON {
        1.0
    } else {
        global_max - global_min
    };

    let colors = [
        "#2196F3", "#4CAF50", "#FF9800", "#E91E63",
        "#9C27B0", "#00BCD4",
    ];

    let mut svg = String::new();
    let _ = write!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{svg_width}" height="{svg_height}">"#,
    );

    // Background
    let _ = write!(
        svg,
        "<rect width=\"{svg_width}\" height=\"{svg_height}\" fill=\"#fafafa\"/>",
    );

    // Title
    if !config.title.is_empty() {
        let _ = write!(
            svg,
            r#"<text x="{}" y="20" text-anchor="middle" font-size="14" font-weight="bold">{}</text>"#,
            svg_width / 2,
            html_escape(&config.title),
        );
    }

    // Y-axis label
    if !config.y_label.is_empty() {
        let _ = write!(
            svg,
            r#"<text x="15" y="{}" text-anchor="middle" font-size="11" transform="rotate(-90,15,{})">{}</text>"#,
            margin + plot_h / 2,
            margin + plot_h / 2,
            html_escape(&config.y_label),
        );
    }

    // Y-axis gridlines and labels
    let grid_count = 5;
    for i in 0..=grid_count {
        let y = margin + (plot_h * i / grid_count);
        let val = global_max - (range * i as f64 / grid_count as f64);
        let _ = write!(
            svg,
            "<line x1=\"{margin}\" y1=\"{y}\" x2=\"{}\" y2=\"{y}\" stroke=\"#ddd\"/>",
            margin + plot_w,
        );
        let _ = write!(
            svg,
            r#"<text x="{}" y="{}" text-anchor="end" font-size="10">{:.0}</text>"#,
            margin - 5,
            y + 4,
            val,
        );
    }

    // Plot each series
    for (si, s) in series.iter().enumerate() {
        if s.points.is_empty() {
            continue;
        }
        let color = colors[si % colors.len()];
        let n = s.points.len();

        let mut path = String::new();
        for (i, point) in s.points.iter().enumerate() {
            let x = if n > 1 {
                margin + (plot_w * i / (n - 1))
            } else {
                margin + plot_w / 2
            };
            let y_norm = (point.value - global_min) / range;
            let y = margin + plot_h
                - (y_norm * plot_h as f64).round() as usize;

            if i == 0 {
                let _ = write!(path, "M{x},{y}");
            } else {
                let _ = write!(path, " L{x},{y}");
            }

            // Data point dot
            let _ = write!(
                svg,
                r#"<circle cx="{x}" cy="{y}" r="3" fill="{color}"/>"#,
            );
        }

        let _ = write!(
            svg,
            r#"<path d="{path}" fill="none" stroke="{color}" stroke-width="2"/>"#,
        );
    }

    // X-axis labels
    if let Some(first_series) = series.first() {
        let n = first_series.points.len();
        let label_step = if n > 10 { n / 8 } else { 1 };
        for (i, point) in first_series.points.iter().enumerate() {
            if i % label_step != 0 && i != n - 1 {
                continue;
            }
            let x = if n > 1 {
                margin + (plot_w * i / (n - 1))
            } else {
                margin + plot_w / 2
            };
            let _ = write!(
                svg,
                r#"<text x="{x}" y="{}" text-anchor="middle" font-size="9">{}</text>"#,
                margin + plot_h + 15,
                html_escape(&point.label),
            );
        }
    }

    // Legend
    let legend_y = svg_height - 15;
    let mut legend_x = margin;
    for (si, s) in series.iter().enumerate() {
        let color = colors[si % colors.len()];
        let _ = write!(
            svg,
            r#"<rect x="{legend_x}" y="{}" width="10" height="10" fill="{color}"/>"#,
            legend_y - 8,
        );
        let _ = write!(
            svg,
            r#"<text x="{}" y="{legend_y}" font-size="10">{}</text>"#,
            legend_x + 14,
            html_escape(&s.name),
        );
        legend_x += 14 + s.name.len() * 7 + 20;
    }

    let _ = write!(svg, "</svg>");

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>{title}</title>
<style>
body {{ font-family: sans-serif; margin: 20px; background: #fff; }}
.chart {{ margin: 20px 0; }}
table {{ border-collapse: collapse; margin: 20px 0; }}
th, td {{ border: 1px solid #ddd; padding: 8px; text-align: right; }}
th {{ background: #f5f5f5; }}
</style>
</head>
<body>
<h1>{title}</h1>
<div class="chart">{svg}</div>
</body>
</html>"#,
        title = html_escape(&config.title),
        svg = svg,
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_series() -> Series {
        Series {
            name: "row_count".to_owned(),
            points: vec![
                DataPoint { label: "t=0".to_owned(), value: 1000.0 },
                DataPoint { label: "t=60".to_owned(), value: 1500.0 },
                DataPoint { label: "t=120".to_owned(), value: 2000.0 },
                DataPoint { label: "t=180".to_owned(), value: 1800.0 },
            ],
        }
    }

    fn sample_multi_series() -> Vec<Series> {
        vec![
            Series {
                name: "estimated".to_owned(),
                points: vec![
                    DataPoint { label: "t=0".to_owned(), value: 1000.0 },
                    DataPoint { label: "t=60".to_owned(), value: 1200.0 },
                    DataPoint { label: "t=120".to_owned(), value: 1100.0 },
                ],
            },
            Series {
                name: "actual".to_owned(),
                points: vec![
                    DataPoint { label: "t=0".to_owned(), value: 1000.0 },
                    DataPoint { label: "t=60".to_owned(), value: 1500.0 },
                    DataPoint { label: "t=120".to_owned(), value: 1400.0 },
                ],
            },
        ]
    }

    #[test]
    fn ascii_bar_chart_basic() {
        let s = sample_series();
        let config = ChartConfig {
            title: "Row Counts".to_owned(),
            ..ChartConfig::default()
        };
        let out = render_ascii_bar_chart(&s, &config);
        assert!(out.contains("Row Counts"));
        assert!(out.contains("t=0"));
        assert!(out.contains("t=180"));
        assert!(out.contains("#"));
    }

    #[test]
    fn ascii_bar_chart_empty() {
        let s = Series {
            name: "empty".to_owned(),
            points: vec![],
        };
        let config = ChartConfig::default();
        let out = render_ascii_bar_chart(&s, &config);
        assert!(out.contains("no data"));
    }

    #[test]
    fn ascii_bar_chart_single_point() {
        let s = Series {
            name: "single".to_owned(),
            points: vec![
                DataPoint { label: "t=0".to_owned(), value: 500.0 },
            ],
        };
        let config = ChartConfig::default();
        let out = render_ascii_bar_chart(&s, &config);
        assert!(out.contains("500.0"));
    }

    #[test]
    fn ascii_bar_chart_equal_values() {
        let s = Series {
            name: "flat".to_owned(),
            points: vec![
                DataPoint { label: "a".to_owned(), value: 100.0 },
                DataPoint { label: "b".to_owned(), value: 100.0 },
            ],
        };
        let config = ChartConfig::default();
        let out = render_ascii_bar_chart(&s, &config);
        assert!(out.contains("100.0"));
    }

    #[test]
    fn ascii_sparkline_basic() {
        let series = sample_multi_series();
        let config = ChartConfig {
            title: "Estimates".to_owned(),
            ..ChartConfig::default()
        };
        let out = render_ascii_sparkline(&series, &config);
        assert!(out.contains("Estimates"));
        assert!(out.contains("estimated"));
        assert!(out.contains("actual"));
    }

    #[test]
    fn ascii_sparkline_empty() {
        let out = render_ascii_sparkline(&[], &ChartConfig::default());
        assert!(out.contains("no data"));
    }

    #[test]
    fn ascii_table_basic() {
        let headers = vec!["Time", "Rows", "Q-Error"];
        let rows = vec![
            vec!["0".to_owned(), "1000".to_owned(), "1.0".to_owned()],
            vec!["60".to_owned(), "1500".to_owned(), "1.2".to_owned()],
        ];
        let out = render_ascii_table(&headers, &rows);
        assert!(out.contains("Time"));
        assert!(out.contains("Rows"));
        assert!(out.contains("Q-Error"));
        assert!(out.contains("1000"));
        assert!(out.contains("1500"));
    }

    #[test]
    fn ascii_table_empty_headers() {
        let out = render_ascii_table(&[], &[]);
        assert!(out.is_empty());
    }

    #[test]
    fn ascii_table_empty_rows() {
        let headers = vec!["A", "B"];
        let out = render_ascii_table(&headers, &[]);
        assert!(out.contains("A"));
        assert!(out.contains("B"));
        assert!(out.contains("-"));
    }

    #[test]
    fn html_chart_basic() {
        let series = sample_multi_series();
        let config = ChartConfig {
            title: "Cost Evolution".to_owned(),
            width: 600,
            height: 300,
            ..ChartConfig::default()
        };
        let out = render_html_chart(&series, &config);
        assert!(out.contains("<!DOCTYPE html>"));
        assert!(out.contains("<svg"));
        assert!(out.contains("Cost Evolution"));
        assert!(out.contains("estimated"));
        assert!(out.contains("actual"));
    }

    #[test]
    fn html_chart_single_series() {
        let series = vec![sample_series()];
        let config = ChartConfig {
            title: "Rows".to_owned(),
            width: 500,
            height: 250,
            ..ChartConfig::default()
        };
        let out = render_html_chart(&series, &config);
        assert!(out.contains("<svg"));
        assert!(out.contains("row_count"));
    }

    #[test]
    fn html_chart_empty_series() {
        let config = ChartConfig::default();
        let out = render_html_chart(&[], &config);
        assert!(out.contains("<!DOCTYPE html>"));
        assert!(out.contains("<svg"));
    }

    #[test]
    fn html_chart_escapes_special_chars() {
        let series = vec![Series {
            name: "a<b&c".to_owned(),
            points: vec![
                DataPoint {
                    label: "x<1".to_owned(),
                    value: 1.0,
                },
            ],
        }];
        let config = ChartConfig {
            title: "Test <&>".to_owned(),
            ..ChartConfig::default()
        };
        let out = render_html_chart(&series, &config);
        assert!(out.contains("&lt;"));
        assert!(out.contains("&amp;"));
    }

    #[test]
    fn html_escape_function() {
        assert_eq!(html_escape("<b>hi</b>"), "&lt;b&gt;hi&lt;/b&gt;");
        assert_eq!(html_escape("a&b"), "a&amp;b");
        assert_eq!(html_escape("\"q\""), "&quot;q&quot;");
    }

    #[test]
    fn chart_config_default() {
        let c = ChartConfig::default();
        assert_eq!(c.width, 60);
        assert_eq!(c.height, 20);
        assert_eq!(c.y_label, "Value");
        assert_eq!(c.x_label, "Time");
        assert!(c.title.is_empty());
    }

    #[test]
    fn data_point_fields() {
        let dp = DataPoint {
            label: "t=0".to_owned(),
            value: 42.5,
        };
        assert_eq!(dp.label, "t=0");
        assert!((dp.value - 42.5).abs() < f64::EPSILON);
    }

    #[test]
    fn series_fields() {
        let s = Series {
            name: "test".to_owned(),
            points: vec![],
        };
        assert_eq!(s.name, "test");
        assert!(s.points.is_empty());
    }

    #[test]
    fn ascii_bar_chart_respects_width() {
        let s = sample_series();
        let config = ChartConfig {
            width: 40,
            ..ChartConfig::default()
        };
        let out = render_ascii_bar_chart(&s, &config);
        for line in out.lines() {
            assert!(
                line.len() <= 100,
                "line too long: {line}"
            );
        }
    }

    #[test]
    fn ascii_bar_chart_with_title() {
        let s = sample_series();
        let config = ChartConfig {
            title: "My Chart".to_owned(),
            ..ChartConfig::default()
        };
        let out = render_ascii_bar_chart(&s, &config);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "My Chart");
        assert_eq!(lines[1], "--------");
    }

    #[test]
    fn ascii_bar_chart_without_title() {
        let s = sample_series();
        let config = ChartConfig::default();
        let out = render_ascii_bar_chart(&s, &config);
        assert!(!out.starts_with('\n'));
        assert!(out.contains("t=0"));
    }

    #[test]
    fn sparkline_contains_range() {
        let series = sample_multi_series();
        let config = ChartConfig::default();
        let out = render_ascii_sparkline(&series, &config);
        assert!(out.contains("range:"));
    }

    #[test]
    fn html_has_legend() {
        let series = sample_multi_series();
        let config = ChartConfig::default();
        let out = render_html_chart(&series, &config);
        assert!(out.contains("estimated"));
        assert!(out.contains("actual"));
    }

    #[test]
    fn html_has_gridlines() {
        let series = vec![sample_series()];
        let config = ChartConfig::default();
        let out = render_html_chart(&series, &config);
        assert!(out.contains("stroke=\"#ddd\""));
    }

    #[test]
    fn html_has_data_points() {
        let series = vec![sample_series()];
        let config = ChartConfig::default();
        let out = render_html_chart(&series, &config);
        assert!(out.contains("<circle"));
        assert!(out.contains("<path"));
    }

    #[test]
    fn ascii_table_alignment() {
        let headers = vec!["Name", "Value"];
        let rows = vec![
            vec!["short".to_owned(), "1".to_owned()],
            vec!["much longer name".to_owned(), "2".to_owned()],
        ];
        let out = render_ascii_table(&headers, &rows);
        let lines: Vec<&str> = out.lines().collect();
        // Separator line width should match header
        let header_len = lines[0].len();
        let sep_len = lines[1].len();
        assert_eq!(header_len, sep_len);
    }

    #[test]
    fn ascii_bar_large_values() {
        let s = Series {
            name: "big".to_owned(),
            points: vec![
                DataPoint {
                    label: "a".to_owned(),
                    value: 1_000_000.0,
                },
                DataPoint {
                    label: "b".to_owned(),
                    value: 5_000_000.0,
                },
            ],
        };
        let config = ChartConfig::default();
        let out = render_ascii_bar_chart(&s, &config);
        assert!(out.contains("1000000.0"));
        assert!(out.contains("5000000.0"));
    }

    #[test]
    fn ascii_bar_negative_values() {
        let s = Series {
            name: "neg".to_owned(),
            points: vec![
                DataPoint { label: "a".to_owned(), value: -10.0 },
                DataPoint { label: "b".to_owned(), value: 10.0 },
            ],
        };
        let config = ChartConfig::default();
        let out = render_ascii_bar_chart(&s, &config);
        assert!(out.contains("-10.0"));
        assert!(out.contains("10.0"));
    }
}
