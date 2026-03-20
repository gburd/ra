//! Asciinema v2 cast file recorder for TUI sessions.
//!
//! Records terminal state changes as timestamped frames in the
//! asciinema v2 file format (newline-delimited JSON). Output
//! files are compatible with `asciinema play` and can be
//! converted to GIF with `agg`.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::Instant;

use ratatui::backend::TestBackend;
use ratatui::style::Color;
use ratatui::Terminal;

use crate::app::App;
use crate::ui;

/// Errors from the recording process.
#[derive(Debug, thiserror::Error)]
pub enum RecordError {
    /// IO error writing the cast file.
    #[error("recording IO error: {0}")]
    Io(#[from] std::io::Error),
    /// JSON serialization error.
    #[error("recording JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Records TUI frames to an asciinema v2 `.cast` file.
pub struct AsciiRecorder {
    writer: BufWriter<File>,
    start: Instant,
    frame_count: usize,
    width: u16,
    height: u16,
}

impl AsciiRecorder {
    /// Create a new recorder targeting the given output path.
    ///
    /// Writes the asciinema v2 header immediately.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be created or the
    /// header cannot be written.
    pub fn new(
        path: &Path,
        width: u16,
        height: u16,
    ) -> Result<Self, RecordError> {
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);

        let header = serde_json::json!({
            "version": 2,
            "width": width,
            "height": height,
            "env": {
                "SHELL": "/bin/bash",
                "TERM": "xterm-256color"
            },
            "title": "RA Optimizer TUI Recording"
        });

        serde_json::to_writer(&mut writer, &header)?;
        writeln!(writer)?;

        Ok(Self {
            writer,
            start: Instant::now(),
            frame_count: 0,
            width,
            height,
        })
    }

    /// Record a single frame of terminal output.
    ///
    /// # Errors
    ///
    /// Returns an error if the frame cannot be serialized or
    /// written to the output file.
    pub fn record_frame(
        &mut self,
        data: &str,
    ) -> Result<(), RecordError> {
        let elapsed = self.start.elapsed().as_secs_f64();

        let frame =
            serde_json::json!([elapsed, "o", data]);

        serde_json::to_writer(&mut self.writer, &frame)?;
        writeln!(self.writer)?;

        self.frame_count += 1;
        Ok(())
    }

    /// Flush all buffered data to disk.
    ///
    /// # Errors
    ///
    /// Returns an error if the flush fails.
    pub fn flush(&mut self) -> Result<(), RecordError> {
        self.writer.flush()?;
        Ok(())
    }

    /// Number of frames recorded so far.
    #[must_use]
    pub fn frame_count(&self) -> usize {
        self.frame_count
    }

    /// Terminal width used for recording.
    #[must_use]
    pub fn width(&self) -> u16 {
        self.width
    }

    /// Terminal height used for recording.
    #[must_use]
    pub fn height(&self) -> u16 {
        self.height
    }
}

/// Render the current TUI state to an ANSI string.
///
/// Uses a `TestBackend` to capture what ratatui would draw,
/// then converts the buffer cells to a plain-text representation
/// suitable for asciinema recording.
fn render_frame_to_string(
    app: &App,
    width: u16,
    height: u16,
) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend)
        .unwrap_or_else(|e| {
            panic!("failed to create test terminal: {e}")
        });

    terminal
        .draw(|frame| ui::render(frame, app))
        .unwrap_or_else(|e| {
            panic!("failed to render frame: {e}")
        });

    let buffer = terminal.backend().buffer().clone();
    buffer_to_ansi(&buffer, width, height)
}

/// Convert a ratatui `Buffer` into an ANSI-escaped string.
///
/// Iterates row by row, emitting ANSI escape codes for
/// foreground color, background color, and modifiers, then
/// resets at end of each line.
fn buffer_to_ansi(
    buffer: &ratatui::buffer::Buffer,
    width: u16,
    height: u16,
) -> String {
    use ratatui::style::Modifier;
    use std::fmt::Write;

    let mut output = String::with_capacity(
        (width as usize + 2) * height as usize * 4,
    );

    for y in 0..height {
        if y > 0 {
            output.push_str("\r\n");
        }

        for x in 0..width {
            let cell = buffer.cell((x, y));
            let Some(cell) = cell else {
                output.push(' ');
                continue;
            };

            let mut codes: Vec<u8> = Vec::new();

            if cell
                .modifier
                .contains(Modifier::BOLD)
            {
                codes.push(1);
            }
            if cell
                .modifier
                .contains(Modifier::DIM)
            {
                codes.push(2);
            }
            if cell
                .modifier
                .contains(Modifier::ITALIC)
            {
                codes.push(3);
            }
            if cell
                .modifier
                .contains(Modifier::UNDERLINED)
            {
                codes.push(4);
            }
            if cell
                .modifier
                .contains(Modifier::REVERSED)
            {
                codes.push(7);
            }

            if let Some(fg) = color_to_ansi_fg(cell.fg) {
                codes.push(fg);
            }
            if let Some(bg) = color_to_ansi_bg(cell.bg) {
                codes.push(bg);
            }

            if codes.is_empty() {
                let _ = write!(output, "{}", cell.symbol());
            } else {
                let code_str: Vec<String> = codes
                    .iter()
                    .map(|c| c.to_string())
                    .collect();
                let _ = write!(
                    output,
                    "\x1b[{}m{}\x1b[0m",
                    code_str.join(";"),
                    cell.symbol()
                );
            }
        }
    }

    output
}

/// Map a ratatui `Color` to an ANSI foreground code.
fn color_to_ansi_fg(color: Color) -> Option<u8> {
    match color {
        Color::Reset => None,
        Color::Black => Some(30),
        Color::Red => Some(31),
        Color::Green => Some(32),
        Color::Yellow => Some(33),
        Color::Blue => Some(34),
        Color::Magenta => Some(35),
        Color::Cyan => Some(36),
        Color::Gray | Color::White => Some(37),
        Color::DarkGray => Some(90),
        Color::LightRed => Some(91),
        Color::LightGreen => Some(92),
        Color::LightYellow => Some(93),
        Color::LightBlue => Some(94),
        Color::LightMagenta => Some(95),
        Color::LightCyan => Some(96),
        _ => None,
    }
}

/// Map a ratatui `Color` to an ANSI background code.
fn color_to_ansi_bg(color: Color) -> Option<u8> {
    match color {
        Color::Reset => None,
        Color::Black => Some(40),
        Color::Red => Some(41),
        Color::Green => Some(42),
        Color::Yellow => Some(43),
        Color::Blue => Some(44),
        Color::Magenta => Some(45),
        Color::Cyan => Some(46),
        Color::Gray | Color::White => Some(47),
        Color::DarkGray => Some(100),
        Color::LightRed => Some(101),
        Color::LightGreen => Some(102),
        Color::LightYellow => Some(103),
        Color::LightBlue => Some(104),
        Color::LightMagenta => Some(105),
        Color::LightCyan => Some(106),
        _ => None,
    }
}

/// Record a full TUI session to a `.cast` file.
///
/// Steps through every snapshot in the timeline, rendering
/// each frame and writing it to the recorder with a synthetic
/// delay between frames.
///
/// # Errors
///
/// Returns an error on IO or rendering failure.
pub fn record_session(
    app: &mut App,
    output_path: &Path,
    width: u16,
    height: u16,
    frame_delay_secs: f64,
) -> Result<usize, RecordError> {
    let mut recorder =
        AsciiRecorder::new(output_path, width, height)?;

    // Record initial frame
    let frame_data =
        render_frame_to_string(app, width, height);
    recorder.record_frame(&frame_data)?;

    // Step through remaining snapshots
    let total_steps = app.timeline.len();
    for step in 1..total_steps {
        app.current_step = step;
        app.scroll_offset = 0;

        // Use synthetic timestamps for consistent playback
        std::thread::sleep(
            std::time::Duration::from_secs_f64(
                frame_delay_secs,
            ),
        );

        let frame_data =
            render_frame_to_string(app, width, height);
        recorder.record_frame(&frame_data)?;
    }

    recorder.flush()?;
    Ok(recorder.frame_count())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::Timeline;
    use std::path::PathBuf;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("ra-tui-test-{name}.cast"))
    }

    #[test]
    fn recorder_writes_header() {
        let path = temp_path("header");
        {
            let mut rec =
                AsciiRecorder::new(&path, 80, 24)
                    .unwrap_or_else(|e| {
                        panic!(
                            "failed to create recorder: {e}"
                        )
                    });
            rec.flush().unwrap_or_else(|e| {
                panic!("failed to flush: {e}")
            });
        }

        let content = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| {
                panic!("failed to read file: {e}")
            });

        assert!(content.contains("\"version\":2"));
        assert!(content.contains("\"width\":80"));
        assert!(content.contains("\"height\":24"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn recorder_writes_frames() {
        let path = temp_path("frames");
        {
            let mut rec =
                AsciiRecorder::new(&path, 80, 24)
                    .unwrap_or_else(|e| {
                        panic!(
                            "failed to create recorder: {e}"
                        )
                    });
            rec.record_frame("frame 1")
                .unwrap_or_else(|e| {
                    panic!("failed to write frame: {e}")
                });
            rec.record_frame("frame 2")
                .unwrap_or_else(|e| {
                    panic!("failed to write frame: {e}")
                });
            rec.flush().unwrap_or_else(|e| {
                panic!("failed to flush: {e}")
            });
            assert_eq!(rec.frame_count(), 2);
        }

        let content = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| {
                panic!("failed to read file: {e}")
            });
        let lines: Vec<&str> =
            content.lines().collect();
        // header + 2 frames
        assert_eq!(lines.len(), 3);
        assert!(lines[1].contains("frame 1"));
        assert!(lines[2].contains("frame 2"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn recorder_dimensions() {
        let path = temp_path("dims");
        let rec =
            AsciiRecorder::new(&path, 120, 40)
                .unwrap_or_else(|e| {
                    panic!(
                        "failed to create recorder: {e}"
                    )
                });
        assert_eq!(rec.width(), 120);
        assert_eq!(rec.height(), 40);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn render_frame_produces_output() {
        let tl = Timeline::demo();
        let app = App::new(tl).unwrap_or_else(|e| {
            panic!("app creation failed: {e}")
        });
        let output =
            render_frame_to_string(&app, 120, 40);
        assert!(!output.is_empty());
        // Strip ANSI escape codes for content check
        let plain: String = strip_ansi(&output);
        assert!(
            plain.contains("PAUSE"),
            "expected 'PAUSE' in stripped output"
        );
    }

    /// Strip ANSI escape sequences for test assertions.
    fn strip_ansi(s: &str) -> String {
        let mut result = String::with_capacity(s.len());
        let mut chars = s.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\x1b' {
                // Skip ESC [ ... m sequences
                if chars.peek() == Some(&'[') {
                    chars.next();
                    for c in chars.by_ref() {
                        if c.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
            } else {
                result.push(ch);
            }
        }
        result
    }

    #[test]
    fn record_session_creates_file() {
        let tl = Timeline::demo();
        let mut app = App::new(tl).unwrap_or_else(|e| {
            panic!("app creation failed: {e}")
        });
        let path = temp_path("session");

        let frame_count = record_session(
            &mut app, &path, 120, 40, 0.0,
        )
        .unwrap_or_else(|e| {
            panic!("recording failed: {e}")
        });

        assert_eq!(frame_count, 5); // demo has 5 snapshots
        assert!(path.exists());

        let content = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| {
                panic!("failed to read file: {e}")
            });
        let lines: Vec<&str> =
            content.lines().collect();
        // header + 5 frames
        assert_eq!(lines.len(), 6);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn color_to_ansi_fg_maps_basic_colors() {
        assert_eq!(
            color_to_ansi_fg(ratatui::style::Color::Red),
            Some(31)
        );
        assert_eq!(
            color_to_ansi_fg(ratatui::style::Color::Green),
            Some(32)
        );
        assert_eq!(
            color_to_ansi_fg(ratatui::style::Color::Reset),
            None
        );
    }

    #[test]
    fn color_to_ansi_bg_maps_basic_colors() {
        assert_eq!(
            color_to_ansi_bg(ratatui::style::Color::Red),
            Some(41)
        );
        assert_eq!(
            color_to_ansi_bg(ratatui::style::Color::Reset),
            None
        );
    }
}
