//! SQL error formatting with contextual help.

use std::fmt::Write;

use colored::Colorize;

/// Format SQL parsing errors in Rust compiler style
/// with helpful pointers.
pub fn format_sql_error(
    err: &ra_parser::SqlConversionError,
    sql: &str,
) -> anyhow::Error {
    let error_msg = err.to_string();

    let (line_num, col_num) = extract_error_position(&error_msg);

    if let (Some(line), Some(col)) = (line_num, col_num) {
        format_error_with_location(sql, line, col, &error_msg)
    } else if let Some(position) = extract_position(&error_msg) {
        // Handle "at position X" format by converting to line/column
        let (line, col) = position_to_line_col(sql, position);
        format_error_with_location(sql, line, col, &error_msg)
    } else {
        format_error_with_context(sql, &error_msg)
    }
}

/// Extract position from "at position X" error messages.
fn extract_position(error_msg: &str) -> Option<usize> {
    if let Some(pos_start) = error_msg.find("at position ") {
        let rest = &error_msg[pos_start + 12..]; // "at position ".len() = 12
        if let Some(end) = rest.find(|c: char| !c.is_ascii_digit()) {
            rest[..end].parse().ok()
        } else {
            rest.parse().ok()
        }
    } else {
        None
    }
}

/// Extract line and column from sqlparser error message.
fn extract_error_position(
    error_msg: &str,
) -> (Option<usize>, Option<usize>) {
    let mut line = None;
    let mut col = None;

    // Handle "Line: X, Column: Y" format
    if let Some(line_start) = error_msg.find("Line:") {
        if let Some(line_end) = error_msg[line_start..].find(',') {
            if let Ok(num) = error_msg
                [line_start + 5..line_start + line_end]
                .trim()
                .parse::<usize>()
            {
                line = Some(num);
            }
        }
    }

    if let Some(col_start) = error_msg.find("Column:") {
        let rest = &error_msg[col_start + 7..];
        if let Some(end) = rest.find(|c: char| !c.is_numeric()) {
            if let Ok(num) = rest[..end].trim().parse::<usize>() {
                col = Some(num);
            }
        } else if let Ok(num) = rest.trim().parse::<usize>() {
            col = Some(num);
        }
    }

    (line, col)
}

/// Convert position to line and column given the SQL text.
fn position_to_line_col(sql: &str, position: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;

    for (i, ch) in sql.chars().enumerate() {
        if i >= position {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }

    (line, col)
}

/// Provide contextual help based on the specific error type.
fn format_contextual_help(
    error_msg: &str,
    error_line: &str,
    _col_idx: usize,
) -> String {
    let mut help = String::new();

    if error_msg.contains("expected: an expression")
        && error_msg.contains("found: {")
    {
        let _ = write!(help, "{}: ", "help".green().bold());
        help.push_str("JSON literals must be quoted strings\n");
        let _ = writeln!(
            help,
            "      {} Use '{{\"key\": \"value\"}}' instead of \
             {{key: value}}",
            "|".blue()
        );
        let _ = writeln!(
            help,
            "      {} In bash, escape quotes: \
             '\\'{{...}}\\'' or use $'...' syntax",
            "|".blue()
        );
    } else if error_line.contains("@=") {
        let _ = write!(help, "{}: ", "help".green().bold());
        help.push_str(
            "@= is not a standard PostgreSQL operator\n",
        );
        let _ = writeln!(
            help,
            "      {} Use @> (contains) or @? (path exists) \
             instead",
            "|".blue()
        );
        let _ = writeln!(
            help,
            "      {} Example: WHERE data @> \
             '{{\"status\": \"active\"}}'",
            "|".blue()
        );
    } else if error_msg.contains("found: @")
        && !error_line.contains("@@")
    {
        let _ = write!(help, "{}: ", "help".green().bold());
        help.push_str("Check PostgreSQL operator syntax\n");
        let _ = writeln!(
            help,
            "      {} Supported JSONB operators: @> <@ @? @@",
            "|".blue()
        );
        let _ = writeln!(
            help,
            "      {} Supported text operators: @@ \
             (tsvector match)",
            "|".blue()
        );
    } else if error_msg.contains("unterminated")
        || error_line
            .chars()
            .filter(|&c| c == '\'')
            .count()
            % 2
            != 0
    {
        let _ = write!(help, "{}: ", "help".green().bold());
        help.push_str("Check string quote matching\n");
        let _ = writeln!(
            help,
            "      {} SQL strings use single quotes: 'text'",
            "|".blue()
        );
        let _ = writeln!(
            help,
            "      {} Escape quotes in bash: \
             '\\''text'\\'' or \"'text'\"",
            "|".blue()
        );
    } else if !error_msg.contains("unsupported") {
        let _ = write!(help, "{}: ", "help".green().bold());
        help.push_str("Check SQL syntax\n");
        let _ = writeln!(
            help,
            "      {} Ensure proper quoting and operator usage",
            "|".blue()
        );
        let _ = writeln!(
            help,
            "      {} Set DEBUG_RA=2 for full error details",
            "|".blue()
        );
    }

    help
}

/// Format error with precise line and column location.
fn format_error_with_location(
    sql: &str,
    line_num: usize,
    col_num: usize,
    error_msg: &str,
) -> anyhow::Error {
    let lines: Vec<&str> = sql.lines().collect();
    let line_idx = line_num.saturating_sub(1);

    let debug_level = std::env::var("DEBUG_RA")
        .or_else(|_| std::env::var("RA_DEBUG"))
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(0);

    if line_idx >= lines.len() {
        let msg = format!("SQL parse error: {error_msg}");
        return if debug_level > 1 {
            anyhow::anyhow!("{msg}")
        } else {
            anyhow::Error::msg(msg)
        };
    }

    let error_line = lines[line_idx];
    let col_idx =
        col_num.saturating_sub(1).clamp(0, error_line.len());

    let mut output = String::new();
    let _ = writeln!(
        output,
        "{}: SQL parse error",
        "error".red().bold()
    );
    let _ = writeln!(
        output,
        "  {} {}",
        "-->".blue().bold(),
        "query:".dimmed()
    );
    output.push('\n');

    if line_idx > 0 {
        let _ = writeln!(
            output,
            "{} {} {}",
            format!("{:4}", line_num - 1).blue().bold(),
            "|".blue().bold(),
            lines[line_idx - 1].dimmed()
        );
    }

    let _ = writeln!(
        output,
        "{} {} {}",
        format!("{line_num:4}").blue().bold(),
        "|".blue().bold(),
        error_line
    );

    let pointer_padding = col_idx;
    let caret_len =
        (error_line.len() - col_idx).clamp(1, 10);
    let _ = writeln!(
        output,
        "     {} {}{} {}",
        "|".blue().bold(),
        " ".repeat(pointer_padding),
        "^".repeat(caret_len).red().bold(),
        error_msg.red()
    );

    if line_idx + 1 < lines.len() {
        let _ = writeln!(
            output,
            "{} {} {}",
            format!("{:4}", line_num + 1).blue().bold(),
            "|".blue().bold(),
            lines[line_idx + 1].dimmed()
        );
    }

    output.push('\n');
    output.push_str(&format_contextual_help(
        error_msg, error_line, col_idx,
    ));

    if debug_level > 1 {
        anyhow::anyhow!("{output}")
    } else {
        anyhow::Error::msg(output)
    }
}

/// Format error with general context highlighting.
fn format_error_with_context(
    sql: &str,
    error_msg: &str,
) -> anyhow::Error {
    let mut output = String::new();
    let _ = writeln!(
        output,
        "{}: SQL parse error",
        "error".red().bold()
    );
    let _ = writeln!(
        output,
        "  {} {}",
        "-->".blue().bold(),
        "query:".dimmed()
    );
    output.push('\n');

    for (i, line) in sql.lines().enumerate() {
        let _ = writeln!(
            output,
            "{} {} {}",
            format!("{:4}", i + 1).blue().bold(),
            "|".blue().bold(),
            line
        );
    }

    let _ = writeln!(
        output,
        "\n{}: {}",
        "error".red().bold(),
        error_msg
    );

    output.push('\n');
    for line in sql.lines() {
        if !line.trim().is_empty() {
            output.push_str(&format_contextual_help(
                error_msg, line, 0,
            ));
            break;
        }
    }

    let debug_level = std::env::var("DEBUG_RA")
        .or_else(|_| std::env::var("RA_DEBUG"))
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(0);

    if debug_level > 1 {
        anyhow::anyhow!("{output}")
    } else {
        anyhow::Error::msg(output)
    }
}
