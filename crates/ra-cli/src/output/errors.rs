//! SQL error formatting with contextual help.

use colored::Colorize;

/// Format SQL parsing errors in Rust compiler style with helpful pointers.
pub fn format_sql_error(
    err: &ra_parser::SqlConversionError,
    sql: &str,
) -> anyhow::Error {
    let error_msg = err.to_string();

    let (line_num, col_num) = extract_error_position(&error_msg);

    if let (Some(line), Some(col)) = (line_num, col_num) {
        format_error_with_location(sql, line, col, &error_msg)
    } else {
        format_error_with_context(sql, &error_msg)
    }
}

/// Extract line and column from sqlparser error message.
fn extract_error_position(
    error_msg: &str,
) -> (Option<usize>, Option<usize>) {
    let mut line = None;
    let mut col = None;

    if let Some(line_start) = error_msg.find("Line:") {
        if let Some(line_end) = error_msg[line_start..].find(',') {
            if let Ok(num) = error_msg[line_start + 5..line_start + line_end]
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
        help.push_str(&format!("{}: ", "help".green().bold()));
        help.push_str("JSON literals must be quoted strings\n");
        help.push_str(&format!(
            "      {} Use '{{\"key\": \"value\"}}' instead of {{key: value}}\n",
            "|".blue()
        ));
        help.push_str(&format!(
            "      {} In bash, escape quotes: '\\'{{...}}\\'' or use $'...' syntax\n",
            "|".blue()
        ));
    } else if error_line.contains("@=") {
        help.push_str(&format!("{}: ", "help".green().bold()));
        help.push_str("@= is not a standard PostgreSQL operator\n");
        help.push_str(&format!(
            "      {} Use @> (contains) or @? (path exists) instead\n",
            "|".blue()
        ));
        help.push_str(&format!(
            "      {} Example: WHERE data @> '{{\"status\": \"active\"}}'\n",
            "|".blue()
        ));
    } else if error_msg.contains("found: @") && !error_line.contains("@@") {
        help.push_str(&format!("{}: ", "help".green().bold()));
        help.push_str("Check PostgreSQL operator syntax\n");
        help.push_str(&format!(
            "      {} Supported JSONB operators: @> <@ @? @@\n",
            "|".blue()
        ));
        help.push_str(&format!(
            "      {} Supported text operators: @@ (tsvector match)\n",
            "|".blue()
        ));
    } else if error_msg.contains("unterminated")
        || error_line.chars().filter(|&c| c == '\'').count() % 2 != 0
    {
        help.push_str(&format!("{}: ", "help".green().bold()));
        help.push_str("Check string quote matching\n");
        help.push_str(&format!(
            "      {} SQL strings use single quotes: 'text'\n",
            "|".blue()
        ));
        help.push_str(&format!(
            "      {} Escape quotes in bash: '\\''text'\\'' or \"'text'\"\n",
            "|".blue()
        ));
    } else if !error_msg.contains("unsupported") {
        help.push_str(&format!("{}: ", "help".green().bold()));
        help.push_str("Check SQL syntax\n");
        help.push_str(&format!(
            "      {} Ensure proper quoting and operator usage\n",
            "|".blue()
        ));
        help.push_str(&format!(
            "      {} Set DEBUG_RA=2 for full error details\n",
            "|".blue()
        ));
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
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(0);

    if line_idx >= lines.len() {
        let msg = format!("SQL parse error: {}", error_msg);
        return if debug_level > 1 {
            anyhow::anyhow!("{}", msg)
        } else {
            anyhow::Error::msg(msg)
        };
    }

    let error_line = lines[line_idx];
    let col_idx = col_num.saturating_sub(1).min(error_line.len());

    let mut output = String::new();
    output.push_str(&format!(
        "{}: SQL parse error\n",
        "error".red().bold()
    ));
    output.push_str(&format!(
        "  {} {}\n",
        "-->".blue().bold(),
        "query:".dimmed()
    ));
    output.push('\n');

    if line_idx > 0 {
        output.push_str(&format!(
            "{} {} {}\n",
            format!("{:4}", line_num - 1).blue().bold(),
            "|".blue().bold(),
            lines[line_idx - 1].dimmed()
        ));
    }

    output.push_str(&format!(
        "{} {} {}\n",
        format!("{:4}", line_num).blue().bold(),
        "|".blue().bold(),
        error_line
    ));

    let pointer_padding = col_idx;
    output.push_str(&format!(
        "     {} {}{} {}\n",
        "|".blue().bold(),
        " ".repeat(pointer_padding),
        "^".repeat((error_line.len() - col_idx).min(10).max(1))
            .red()
            .bold(),
        error_msg.red()
    ));

    if line_idx + 1 < lines.len() {
        output.push_str(&format!(
            "{} {} {}\n",
            format!("{:4}", line_num + 1).blue().bold(),
            "|".blue().bold(),
            lines[line_idx + 1].dimmed()
        ));
    }

    output.push_str("\n");
    output.push_str(&format_contextual_help(error_msg, error_line, col_idx));

    if debug_level > 1 {
        anyhow::anyhow!("{}", output)
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
    output.push_str(&format!(
        "{}: SQL parse error\n",
        "error".red().bold()
    ));
    output.push_str(&format!(
        "  {} {}\n",
        "-->".blue().bold(),
        "query:".dimmed()
    ));
    output.push('\n');

    for (i, line) in sql.lines().enumerate() {
        output.push_str(&format!(
            "{} {} {}\n",
            format!("{:4}", i + 1).blue().bold(),
            "|".blue().bold(),
            line
        ));
    }

    output.push_str(&format!(
        "\n{}: {}\n",
        "error".red().bold(),
        error_msg
    ));

    output.push('\n');
    for line in sql.lines() {
        if !line.trim().is_empty() {
            output.push_str(&format_contextual_help(error_msg, line, 0));
            break;
        }
    }

    let debug_level = std::env::var("DEBUG_RA")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(0);

    if debug_level > 1 {
        anyhow::anyhow!("{}", output)
    } else {
        anyhow::Error::msg(output)
    }
}
