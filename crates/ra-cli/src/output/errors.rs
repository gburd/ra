//! SQL error formatting with contextual help.

use std::fmt::Write;

use colored::Colorize;
use ra_parser::StructuredParseError;

/// Format SQL parsing errors in Rust compiler style
/// with helpful pointers.
pub fn format_sql_error(
    err: &ra_parser::SqlConversionError,
    sql: &str,
) -> anyhow::Error {
    // Handle structured errors with precise position info.
    if let ra_parser::SqlConversionError::StructuredParseErrors(
        ref errors,
    ) = err
    {
        return format_structured_errors(sql, errors);
    }

    // Fallback: heuristic-based error display for string errors.
    let error_msg = err.to_string();

    let (line_num, col_num) = extract_error_position(&error_msg);

    if let (Some(line), Some(col)) = (line_num, col_num) {
        format_error_with_location(sql, line, col, &error_msg)
    } else if let Some(position) = extract_position(&error_msg) {
        let (line, col) = position_to_line_col(sql, position);
        format_error_with_location(sql, line, col, &error_msg)
    } else {
        format_error_with_context(sql, &error_msg)
    }
}

/// Format structured parse errors with precise positions and
/// expected-token hints.
fn format_structured_errors(
    sql: &str,
    errors: &[StructuredParseError],
) -> anyhow::Error {
    let lines: Vec<&str> = sql.lines().collect();
    let mut output = String::new();

    let debug_level = std::env::var("DEBUG_RA")
        .or_else(|_| std::env::var("RA_DEBUG"))
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(0);

    for error in errors {
        let (line_num, col_num) =
            position_to_line_col(sql, error.position);
        let line_idx = line_num.saturating_sub(1);
        let carets = error.token_length.max(1);

        // Header
        let _ = writeln!(
            output,
            "{}: {}",
            "error".red().bold(),
            error.message.bold()
        );

        // Location
        let _ = writeln!(
            output,
            "  {} query:{}:{}",
            "-->".blue().bold(),
            line_num,
            col_num
        );

        let _ = writeln!(output, "   {}", "|".blue().bold());

        if line_idx < lines.len() {
            let error_line = lines[line_idx];
            let col_idx =
                col_num.saturating_sub(1).min(error_line.len());

            // Context: up to 2 lines above
            let context_start = line_idx.saturating_sub(2);
            for ctx_idx in context_start..line_idx {
                let _ = writeln!(
                    output,
                    "{} {} {}",
                    format!("{:4}", ctx_idx + 1).blue().bold(),
                    "|".blue().bold(),
                    lines[ctx_idx].dimmed()
                );
            }

            // The error line
            let _ = writeln!(
                output,
                "{} {} {}",
                format!("{line_num:4}").blue().bold(),
                "|".blue().bold(),
                error_line
            );

            // Caret line with token name annotation
            let annotation = if let Some(ref text) = error.token_text {
                format!("unexpected {} '{text}'", error.token_name)
            } else if error.token_name.is_empty() {
                error.message.clone()
            } else {
                format!("unexpected {}", error.token_name)
            };
            let _ = writeln!(
                output,
                "     {} {}{}  {}",
                "|".blue().bold(),
                " ".repeat(col_idx),
                "^".repeat(carets).red().bold(),
                annotation.red()
            );

            // Context: 1 line below
            if line_idx + 1 < lines.len() {
                let _ = writeln!(
                    output,
                    "{} {} {}",
                    format!("{:4}", line_num + 1).blue().bold(),
                    "|".blue().bold(),
                    lines[line_idx + 1].dimmed()
                );
            }

            let _ = writeln!(output, "   {}", "|".blue().bold());

            // Expected tokens hint
            if !error.expected_tokens.is_empty() {
                let _ = write!(
                    output,
                    "{}: ",
                    "help".green().bold()
                );
                let _ = writeln!(
                    output,
                    "expected one of: {}",
                    error.expected_tokens.join(", ")
                );
            }

            // Contextual help from heuristics (secondary)
            output.push_str(&format_contextual_help(
                &error.message,
                error_line,
                col_idx,
            ));
        }

        output.push('\n');
    }

    if debug_level > 1 {
        anyhow::anyhow!("{output}")
    } else {
        anyhow::Error::msg(output)
    }
}

/// Extract position from "at position X" error messages.
fn extract_position(error_msg: &str) -> Option<usize> {
    if let Some(pos_start) = error_msg.find("at position ") {
        let rest = &error_msg[pos_start + 12..];
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

/// Convert a byte position to line and column numbers.
///
/// The lexer provides byte offsets, so we iterate over bytes
/// counting newlines for line numbers and bytes since the last
/// newline for column.
fn position_to_line_col(sql: &str, position: usize) -> (usize, usize) {
    let mut line = 1;
    let mut line_start = 0;
    for (i, byte) in sql.bytes().enumerate() {
        if i >= position {
            break;
        }
        if byte == b'\n' {
            line += 1;
            line_start = i + 1;
        }
    }
    let col = position - line_start + 1;
    (line, col)
}

/// Extract the problematic token/character from an error message.
///
/// Looks for patterns like:
/// - "unexpected character 'X'"
/// - "unexpected token 'XYZ'"
/// - "unterminated string literal"
/// - "invalid float literal 'X'"
fn extract_token(error_msg: &str) -> Option<&str> {
    for prefix in &[
        "unexpected character '",
        "unexpected token '",
        "invalid float literal '",
        "invalid integer literal '",
    ] {
        if let Some(start) = error_msg.find(prefix) {
            let after = &error_msg[start + prefix.len()..];
            if let Some(end) = after.find('\'') {
                return Some(&after[..end]);
            }
        }
    }
    None
}

/// Strip redundant prefixes and position suffixes from the error
/// message for display as an annotation.
fn clean_annotation(error_msg: &str) -> String {
    let mut msg = error_msg.to_string();

    // Strip thiserror-generated prefixes
    for prefix in &[
        "failed to parse SQL: ",
        "unsupported SQL feature: ",
        "invalid SQL: ",
        "invalid recursive CTE: ",
    ] {
        if let Some(stripped) = msg.strip_prefix(prefix) {
            msg = stripped.to_string();
        }
    }

    // Strip " at position N" suffix
    if let Some(pos) = msg.find(" at position ") {
        msg.truncate(pos);
    }

    msg
}

/// Determine a short annotation to display next to the carets.
fn make_caret_annotation(error_msg: &str, error_line: &str, col_idx: usize) -> String {
    // Check if this is a :: cast operator issue
    if error_line.get(col_idx..).is_some_and(|s| s.starts_with("::")) {
        return "the `::` type cast operator is not yet supported"
            .to_string();
    }

    // Check for -> / ->> JSON operators
    if error_line.get(col_idx..).is_some_and(|s| s.starts_with("->>")) {
        return "the `->>` JSON text extraction operator is not yet supported"
            .to_string();
    }
    if error_line.get(col_idx..).is_some_and(|s| s.starts_with("->")) {
        return "the `->` JSON extraction operator is not yet supported"
            .to_string();
    }

    clean_annotation(error_msg)
}

/// Determine the caret length based on the problematic token.
fn caret_length(error_msg: &str, error_line: &str, col_idx: usize) -> usize {
    // For :: cast operator
    if error_line.get(col_idx..).is_some_and(|s| s.starts_with("::")) {
        return 2;
    }
    // For ->> operator
    if error_line.get(col_idx..).is_some_and(|s| s.starts_with("->>")) {
        return 3;
    }
    // For -> operator
    if error_line.get(col_idx..).is_some_and(|s| s.starts_with("->")) {
        return 2;
    }

    // Use the extracted token length if available
    if let Some(token) = extract_token(error_msg) {
        let len = token.len();
        if len > 0 {
            return len;
        }
    }

    // Default: single character
    1
}

/// Provide contextual help based on the specific error type.
fn format_contextual_help(
    error_msg: &str,
    error_line: &str,
    col_idx: usize,
) -> String {
    let mut help = String::new();

    // :: type cast operator
    if error_line.get(col_idx..).is_some_and(|s| s.starts_with("::")) {
        let _ = write!(help, "{}: ", "help".green().bold());
        help.push_str("use CAST() instead\n");
        // Try to build a suggestion from the error line
        if let Some(suggestion) = suggest_cast_replacement(error_line, col_idx) {
            let _ = writeln!(
                help,
                "   {} {}",
                "|".blue(),
                suggestion
            );
        } else {
            let _ = writeln!(
                help,
                "   {} CAST(expression AS type)",
                "|".blue()
            );
        }
        return help;
    }

    // -> / ->> JSON operators
    if error_line.get(col_idx..).is_some_and(|s| s.starts_with("->>") || s.starts_with("->")) {
        let _ = write!(help, "{}: ", "help".green().bold());
        help.push_str("JSON operators are not yet supported\n");
        let _ = writeln!(
            help,
            "   {} Consider restructuring the query to \
             avoid JSON path extraction",
            "|".blue()
        );
        return help;
    }

    // INTERVAL literal
    if error_msg.contains("INTERVAL") || error_line.contains("INTERVAL") {
        let _ = write!(help, "{}: ", "help".green().bold());
        help.push_str("INTERVAL literal syntax is not yet supported\n");
        let _ = writeln!(
            help,
            "   {} Consider using numeric expressions for \
             time calculations",
            "|".blue()
        );
        return help;
    }

    if error_msg.contains("expected: an expression")
        && error_msg.contains("found: {")
    {
        let _ = write!(help, "{}: ", "help".green().bold());
        help.push_str("JSON literals must be quoted strings\n");
        let _ = writeln!(
            help,
            "   {} Use '{{\"key\": \"value\"}}' instead of \
             {{key: value}}",
            "|".blue()
        );
        let _ = writeln!(
            help,
            "   {} In bash, escape quotes: \
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
            "   {} Use @> (contains) or @? (path exists) \
             instead",
            "|".blue()
        );
    } else if error_msg.contains("found: @")
        && !error_line.contains("@@")
    {
        let _ = write!(help, "{}: ", "help".green().bold());
        help.push_str("check PostgreSQL operator syntax\n");
        let _ = writeln!(
            help,
            "   {} Supported JSONB operators: @> <@ @? @@",
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
        help.push_str("check string quote matching\n");
        let _ = writeln!(
            help,
            "   {} SQL strings use single quotes: 'text'",
            "|".blue()
        );
        let _ = writeln!(
            help,
            "   {} Escape quotes in bash: \
             '\\''text'\\'' or \"'text'\"",
            "|".blue()
        );
    } else if !error_msg.contains("unsupported") {
        let _ = write!(help, "{}: ", "help".green().bold());
        help.push_str("check SQL syntax\n");
        let _ = writeln!(
            help,
            "   {} Ensure proper quoting and operator usage",
            "|".blue()
        );
        let _ = writeln!(
            help,
            "   {} Set DEBUG_RA=2 for full error details",
            "|".blue()
        );
    }

    help
}

/// Attempt to suggest a CAST() replacement for a :: type cast.
fn suggest_cast_replacement(line: &str, col_idx: usize) -> Option<String> {
    // Find the type name after ::
    let after_cast = line.get(col_idx + 2..)?;
    let type_end = after_cast
        .find(|c: char| !c.is_alphanumeric() && c != '_')
        .unwrap_or(after_cast.len());
    if type_end == 0 {
        return None;
    }
    let type_name = &after_cast[..type_end];

    // Find the expression before :: by scanning backwards for
    // the matching parenthesized expression or simple identifier
    let before = &line[..col_idx];
    let expr = if before.ends_with(')') {
        // Find matching open paren
        let mut depth = 0i32;
        let mut start = None;
        for (i, ch) in before.char_indices().rev() {
            match ch {
                ')' => depth += 1,
                '(' => {
                    depth -= 1;
                    if depth == 0 {
                        start = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        start.map(|s| &before[s..])
    } else {
        // Simple identifier
        let start = before
            .rfind(|c: char| !c.is_alphanumeric() && c != '_' && c != '.')
            .map(|i| i + 1)
            .unwrap_or(0);
        Some(&before[start..])
    };

    expr.map(|e| format!("CAST({e} AS {type_name})"))
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
        col_num.saturating_sub(1).min(error_line.len());

    let annotation = make_caret_annotation(error_msg, error_line, col_idx);
    let carets = caret_length(error_msg, error_line, col_idx);

    let mut output = String::new();

    // Header: "error: <short description>"
    let short_desc = clean_annotation(error_msg);
    let _ = writeln!(
        output,
        "{}: {}",
        "error".red().bold(),
        short_desc.bold()
    );

    // Location: " --> query:LINE:COL"
    let _ = writeln!(
        output,
        "  {} query:{}:{}",
        "-->".blue().bold(),
        line_num,
        col_num
    );

    // Blank separator line
    let _ = writeln!(output, "   {}", "|".blue().bold());

    // Context: up to 2 lines above
    let context_start = line_idx.saturating_sub(2);
    for ctx_idx in context_start..line_idx {
        let _ = writeln!(
            output,
            "{} {} {}",
            format!("{:4}", ctx_idx + 1).blue().bold(),
            "|".blue().bold(),
            lines[ctx_idx].dimmed()
        );
    }

    // The error line
    let _ = writeln!(
        output,
        "{} {} {}",
        format!("{line_num:4}").blue().bold(),
        "|".blue().bold(),
        error_line
    );

    // Caret line with annotation
    let _ = writeln!(
        output,
        "     {} {}{}  {}",
        "|".blue().bold(),
        " ".repeat(col_idx),
        "^".repeat(carets).red().bold(),
        annotation.red()
    );

    // Context: 1 line below
    if line_idx + 1 < lines.len() {
        let _ = writeln!(
            output,
            "{} {} {}",
            format!("{:4}", line_num + 1).blue().bold(),
            "|".blue().bold(),
            lines[line_idx + 1].dimmed()
        );
    }

    // Blank separator
    let _ = writeln!(output, "   {}", "|".blue().bold());

    // Contextual help
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
///
/// This is the fallback when no position info is available. It
/// tries to locate the error by searching for the problematic
/// token in the SQL text before resorting to showing all lines.
fn format_error_with_context(
    sql: &str,
    error_msg: &str,
) -> anyhow::Error {
    // Try to find the error location by searching for the token
    if let Some(token) = extract_token(error_msg) {
        if let Some(byte_pos) = sql.find(token) {
            let (line, col) = position_to_line_col(sql, byte_pos);
            return format_error_with_location(
                sql, line, col, error_msg,
            );
        }
    }

    // Try to detect :: operator in the SQL
    if error_msg.contains("unexpected character ':'") {
        if let Some(byte_pos) = sql.find("::") {
            let (line, col) = position_to_line_col(sql, byte_pos);
            return format_error_with_location(
                sql, line, col, error_msg,
            );
        }
    }

    // True fallback: show a limited window around the middle
    let lines: Vec<&str> = sql.lines().collect();
    let mut output = String::new();

    let short_desc = clean_annotation(error_msg);
    let _ = writeln!(
        output,
        "{}: {}",
        "error".red().bold(),
        short_desc.bold()
    );
    let _ = writeln!(
        output,
        "  {} query",
        "-->".blue().bold(),
    );
    let _ = writeln!(output, "   {}", "|".blue().bold());

    // Show at most 10 lines: first 5 and last 5, with ellipsis
    let max_display = 10;
    if lines.len() <= max_display {
        for (i, line) in lines.iter().enumerate() {
            let _ = writeln!(
                output,
                "{} {} {}",
                format!("{:4}", i + 1).blue().bold(),
                "|".blue().bold(),
                line
            );
        }
    } else {
        let head = 5;
        let tail = 5;
        for (i, line) in lines.iter().take(head).enumerate() {
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
            " {} {} ...",
            "...".blue().bold(),
            "|".blue().bold()
        );
        let start = lines.len() - tail;
        for (i, line) in lines.iter().skip(start).enumerate() {
            let _ = writeln!(
                output,
                "{} {} {}",
                format!("{:4}", start + i + 1).blue().bold(),
                "|".blue().bold(),
                line
            );
        }
    }

    let _ = writeln!(output, "   {}", "|".blue().bold());

    // Show the full error message
    let _ = writeln!(
        output,
        "   {} {}",
        "=".blue().bold(),
        format!("note: {error_msg}").dimmed()
    );
    output.push('\n');

    // Try to provide some help even without location
    let first_line = lines.first().copied().unwrap_or("");
    output.push_str(&format_contextual_help(
        error_msg, first_line, 0,
    ));

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
