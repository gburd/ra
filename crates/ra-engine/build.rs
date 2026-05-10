//! Build script that compiles .rra rule files into Rust rewrite rules.
//!
//! Scans the `rules/` directory for `.rra` files, extracts YAML frontmatter
//! and `## Implementation` code blocks containing `rw!()` / `rewrite!()`
//! macros, and generates `$OUT_DIR/generated_rules.rs`.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let rules_dir = Path::new(&manifest_dir).join("../../rules");
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("generated_rules.rs");

    // Tell Cargo to rerun if any rule file changes
    if rules_dir.exists() {
        println!("cargo:rerun-if-changed={}", rules_dir.display());
    }

    let rules = scan_rules(&rules_dir);
    let generated = generate_rules_module(&rules);

    let mut file = fs::File::create(&dest_path).unwrap();
    file.write_all(generated.as_bytes()).unwrap();
}

/// Metadata extracted from a .rra file's YAML frontmatter.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct RuleInfo {
    id: String,
    name: String,
    category: String,
    databases: Vec<String>,
    complexity: Option<String>,
    benefit_range: Option<(f64, f64)>,
    /// The extracted `rw!()` / `rewrite!()` code from the Implementation section.
    code_blocks: Vec<String>,
    /// Condition functions referenced (e.g., `single_reference`, `references_only`).
    conditions: Vec<String>,
}

/// Scan the rules directory recursively for .rra files and extract rule info.
fn scan_rules(dir: &Path) -> Vec<RuleInfo> {
    let mut rules = Vec::new();
    if !dir.exists() {
        return rules;
    }
    visit_dir(dir, &mut rules);
    rules
}

fn visit_dir(dir: &Path, rules: &mut Vec<RuleInfo>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            visit_dir(&path, rules);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rra") {
            if let Some(rule) = parse_rra_file(&path) {
                rules.push(rule);
            }
        }
    }
}

/// Parse a single .rra file, extracting frontmatter and Implementation code.
fn parse_rra_file(path: &Path) -> Option<RuleInfo> {
    let contents = fs::read_to_string(path).ok()?;

    // Split on YAML frontmatter delimiters
    let parts: Vec<&str> = contents.splitn(3, "---").collect();
    if parts.len() < 3 {
        return None;
    }

    let frontmatter = parts[1].trim();
    let body = parts[2];

    // Parse frontmatter fields manually (avoid pulling in serde_yml at build time)
    let id = extract_yaml_string(frontmatter, "id")?;
    let name = extract_yaml_string(frontmatter, "name").unwrap_or_else(|| id.clone());
    let category = extract_yaml_string(frontmatter, "category")?;
    let databases = extract_yaml_list(frontmatter, "databases");
    let complexity = extract_yaml_string(frontmatter, "complexity");
    let benefit_range = extract_benefit_range(frontmatter);

    // Extract code from the ## Implementation section
    let code_blocks = extract_implementation_code(body);

    // Skip rules without rewrite macros
    if code_blocks.is_empty() {
        return None;
    }

    // Extract condition function names from `if condition_fn(...)` patterns
    let conditions = extract_conditions(&code_blocks);

    Some(RuleInfo {
        id,
        name,
        category,
        databases,
        complexity,
        benefit_range,
        code_blocks,
        conditions,
    })
}

/// Extract a simple YAML string value for a given key.
fn extract_yaml_string(yaml: &str, key: &str) -> Option<String> {
    for line in yaml.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(key) {
            if let Some(value) = rest.strip_prefix(':') {
                let val = value.trim().trim_matches('"').trim_matches('\'');
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}

/// Extract a YAML list value (inline format: [a, b, c]).
fn extract_yaml_list(yaml: &str, key: &str) -> Vec<String> {
    for line in yaml.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(key) {
            if let Some(value) = rest.strip_prefix(':') {
                let val = value.trim();
                if val.starts_with('[') && val.ends_with(']') {
                    let inner = &val[1..val.len() - 1];
                    return inner
                        .split(',')
                        .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
            }
        }
    }
    Vec::new()
}

/// Extract benefit_range from frontmatter.
fn extract_benefit_range(yaml: &str) -> Option<(f64, f64)> {
    for line in yaml.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("benefit_range") {
            if let Some(value) = rest.strip_prefix(':') {
                let val = value.trim();
                if val.starts_with('[') && val.ends_with(']') {
                    let inner = &val[1..val.len() - 1];
                    let parts: Vec<&str> = inner.split(',').collect();
                    if parts.len() == 2 {
                        let min = parts[0].trim().parse::<f64>().ok()?;
                        let max = parts[1].trim().parse::<f64>().ok()?;
                        return Some((min, max));
                    }
                }
            }
        }
    }
    None
}

/// Extract Rust code blocks from the ## Implementation section.
fn extract_implementation_code(body: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut in_impl_section = false;
    let mut in_code_block = false;
    let mut current_block = String::new();
    let mut code_block_lang: String;

    for line in body.lines() {
        let trimmed = line.trim();

        // Detect section headers
        if trimmed.starts_with("## ") {
            if trimmed == "## Implementation" {
                in_impl_section = true;
            } else if in_impl_section {
                // We've left the Implementation section
                break;
            }
        }

        if !in_impl_section {
            continue;
        }

        // Handle code fences
        if trimmed.starts_with("```") && !in_code_block {
            code_block_lang = trimmed[3..].trim().to_string();
            if code_block_lang == "rust" || code_block_lang.is_empty() {
                in_code_block = true;
                current_block.clear();
            }
        } else if trimmed == "```" && in_code_block {
            in_code_block = false;
            let block = current_block.trim().to_string();
            // Only include blocks that contain rewrite macros
            if block.contains("rw!(") || block.contains("rewrite!(") {
                blocks.push(block);
            }
        } else if in_code_block {
            current_block.push_str(line);
            current_block.push('\n');
        }
    }

    blocks
}

/// Extract condition function names from code blocks.
/// Looks for patterns like `if some_function("?var", "?var2")`
fn extract_conditions(code_blocks: &[String]) -> Vec<String> {
    let mut conditions = Vec::new();
    for block in code_blocks {
        for line in block.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("if ") {
                // Extract the function name before the first '('
                if let Some(paren_idx) = rest.find('(') {
                    let func_name = rest[..paren_idx].trim().to_string();
                    if !func_name.is_empty()
                        && func_name.chars().all(|c| c.is_alphanumeric() || c == '_')
                    {
                        conditions.push(func_name);
                    }
                }
            }
        }
    }
    conditions.sort();
    conditions.dedup();
    conditions
}

/// Check if a rewrite string contains condition functions or custom Appliers.
///
/// Patterns that need special handling:
/// - Multi-line `if condition(...)` (condition function)
/// - Inline `if func("?var")` at end of rewrite
/// - Custom Applier structs `{ StructName { ... } }` in the RHS
fn has_condition_or_applier(rewrite_str: &str) -> bool {
    // Multi-line conditions
    if rewrite_str.contains("\n    if ")
        || rewrite_str.contains("\n        if ")
        || rewrite_str.contains("\n\tif ")
    {
        return true;
    }

    // Inline single-line condition: `"rhs" if func(...)` at end
    // Look for `if ` followed by an identifier and `(` after `=>` or `<=>`
    if let Some(arrow_pos) = rewrite_str.find("=>") {
        let after_arrow = &rewrite_str[arrow_pos..];
        // Find `if ` that's not inside a string literal
        if let Some(if_pos) = after_arrow.rfind(" if ") {
            let after_if = &after_arrow[if_pos + 4..];
            let trimmed = after_if.trim();
            // Check it looks like a function call (identifier + paren)
            if trimmed
                .chars()
                .next()
                .map_or(false, |c| c.is_alphabetic() || c == '_')
                && trimmed.contains('(')
            {
                return true;
            }
        }
    }

    // Custom Applier structs: `{ StructName { field: "?val" } }`
    // Look for `=> { CapitalizedName {` pattern
    if let Some(arrow_pos) = rewrite_str.find("=>") {
        let after_arrow = &rewrite_str[arrow_pos + 2..];
        let trimmed = after_arrow.trim();
        // Pattern: starts with `{` then whitespace then an uppercase letter
        if trimmed.starts_with('{') {
            let inner = trimmed[1..].trim();
            if inner
                .chars()
                .next()
                .map_or(false, |c| c.is_uppercase())
            {
                return true;
            }
        }
        // Pattern: `\n    { StructName {`
        for line in after_arrow.lines() {
            let lt = line.trim();
            if lt.starts_with('{') {
                let inner = lt[1..].trim();
                if inner
                    .chars()
                    .next()
                    .map_or(false, |c| c.is_uppercase())
                {
                    return true;
                }
            }
        }
    }

    false
}

/// Represents an extracted rewrite rule.
enum ExtractedRule {
    /// A single rewrite (uses `=>`)
    Single(String),
    /// A bidirectional rewrite (uses `<=>`), produces Vec<Rewrite>
    Bidirectional(String),
    /// A commented-out rule awaiting condition functions
    Commented(String),
}

/// Normalize a `rw!()` call to `rewrite!()` and clean up the code block.
fn normalize_rewrite_code(block: &str) -> Vec<ExtractedRule> {
    let mut rewrites = Vec::new();

    // Strategy: extract individual rw!/rewrite! invocations using find()
    let normalized = block.replace("rw!(", "rewrite!(");

    // Find each "rewrite!(" occurrence and extract the balanced parens
    let marker = "rewrite!(";
    let mut search_from = 0;

    while let Some(start) = normalized[search_from..].find(marker) {
        let abs_start = search_from + start;
        let after_marker = abs_start + marker.len();

        // Track paren depth starting after the opening paren of rewrite!(
        let mut depth = 1;
        let mut end = None;

        for (byte_offset, ch) in normalized[after_marker..].char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(after_marker + byte_offset + ch.len_utf8());
                        break;
                    }
                }
                _ => {}
            }
        }

        if let Some(end_pos) = end {
            let mut rewrite_str = normalized[abs_start..end_pos].trim().to_string();
            // Remove trailing comma if present
            if rewrite_str.ends_with(',') {
                rewrite_str.pop();
            }
            // Check for condition functions or custom Appliers
            let has_condition = has_condition_or_applier(&rewrite_str);
            if has_condition {
                // Conditional rules — commented out until conditions are implemented
                rewrites.push(ExtractedRule::Commented(format!(
                    "// TODO: requires condition function or custom applier\n        // {}",
                    rewrite_str
                        .lines()
                        .collect::<Vec<_>>()
                        .join("\n        // ")
                )));
            } else if rewrite_str.contains("<=>") {
                // Bidirectional rewrite — produces Vec<Rewrite>
                rewrites.push(ExtractedRule::Bidirectional(rewrite_str));
            } else {
                rewrites.push(ExtractedRule::Single(rewrite_str));
            }
            search_from = end_pos;
        } else {
            // Unbalanced parens — skip
            search_from = after_marker;
        }
    }

    rewrites
}

/// Category key derived from the .rra category field.
/// e.g. "logical/predicate-pushdown" -> "logical_predicate_pushdown"
fn category_to_fn_name(category: &str) -> String {
    category
        .replace('/', "_")
        .replace('-', "_")
        .replace(' ', "_")
        .to_lowercase()
}

/// Generate the complete Rust module from all parsed rules.
fn generate_rules_module(rules: &[RuleInfo]) -> String {
    let mut output = String::with_capacity(64 * 1024);

    output.push_str(
        "// Auto-generated rewrite rules from .rra files.\n\
         //\n\
         // DO NOT EDIT — regenerated by build.rs on each build.\n\
         // Source: rules/**/*.rra\n\
         //\n\
         // Note: This file is include!()-ed into rewrite.rs which already\n\
         // imports egg::{rewrite, Rewrite}, RelAnalysis, and RelLang.\n\
         \n",
    );

    // Group rules by category
    let mut by_category: BTreeMap<String, Vec<&RuleInfo>> = BTreeMap::new();
    for rule in rules {
        by_category
            .entry(rule.category.clone())
            .or_default()
            .push(rule);
    }

    // Generate a function per category
    let mut category_fns = Vec::new();
    let mut total_rules = 0;
    let mut conditional_rules = 0;

    for (category, cat_rules) in &by_category {
        let fn_name = format!("generated_{}_rules", category_to_fn_name(category));
        category_fns.push(fn_name.clone());

        // Collect single rules and bidirectional rules separately
        let mut single_rules: Vec<String> = Vec::new();
        let mut bidir_rules_for_cat: Vec<(String, String)> = Vec::new();
        let mut comments: Vec<String> = Vec::new();

        for rule in cat_rules {
            let rule_comment = format!("// From: {} ({})", rule.id, rule.name);

            for block in &rule.code_blocks {
                let rewrites = normalize_rewrite_code(block);
                for rw in &rewrites {
                    match rw {
                        ExtractedRule::Single(code) => {
                            single_rules.push(format!(
                                "        {}\n        {},",
                                rule_comment, code
                            ));
                            total_rules += 1;
                        }
                        ExtractedRule::Bidirectional(code) => {
                            bidir_rules_for_cat.push((
                                code.clone(),
                                rule.id.clone(),
                            ));
                            total_rules += 2;
                        }
                        ExtractedRule::Commented(code) => {
                            comments.push(format!(
                                "        {}\n        {}",
                                rule_comment, code
                            ));
                            conditional_rules += 1;
                        }
                    }
                }
            }
        }

        output.push_str(&format!(
            "/// Generated rules for category: {}\n",
            category
        ));
        output.push_str(&format!(
            "/// Source: {} .rra files\n",
            cat_rules.len()
        ));
        output.push_str("#[allow(unused)]\n");
        output.push_str(&format!(
            "pub(crate) fn {}() -> Vec<Rewrite<RelLang, RelAnalysis>> {{\n",
            fn_name
        ));

        if bidir_rules_for_cat.is_empty() {
            // Simple case: just a vec![]
            output.push_str("    vec![\n");
            for rule_code in &single_rules {
                output.push_str(rule_code);
                output.push('\n');
            }
            for comment in &comments {
                output.push_str(comment);
                output.push('\n');
            }
            output.push_str("    ]\n");
        } else {
            // Need mut vec + extend for bidirectional rules
            output.push_str("    let mut rules = vec![\n");
            for rule_code in &single_rules {
                output.push_str(rule_code);
                output.push('\n');
            }
            for comment in &comments {
                output.push_str(comment);
                output.push('\n');
            }
            output.push_str("    ];\n");
            for (code, id) in &bidir_rules_for_cat {
                output.push_str(&format!(
                    "    // Bidirectional: {}\n    rules.extend({});\n",
                    id, code
                ));
            }
            output.push_str("    rules\n");
        }

        output.push_str("}\n\n");
    }

    // Generate master function that collects all categories
    output.push_str(&format!(
        "/// All generated rules from .rra files.\n\
         /// Total: {} active rules, {} conditional (awaiting condition functions).\n\
         #[allow(unused)]\n\
         pub(crate) fn all_generated_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {{\n\
         ",
        total_rules, conditional_rules
    ));
    output.push_str(&format!(
        "    let mut rules = Vec::with_capacity({});\n",
        total_rules
    ));
    for fn_name in &category_fns {
        output.push_str(&format!("    rules.extend({}());\n", fn_name));
    }
    output.push_str("    rules\n}\n\n");

    // Generate metadata function
    output.push_str(
        "/// Metadata about generated rules for diagnostics.\n\
         #[allow(unused)]\n\
         pub(crate) fn generated_rule_stats() -> GeneratedRuleStats {\n",
    );
    output.push_str(&format!(
        "    GeneratedRuleStats {{\n\
         \x20       total_rra_files: {},\n\
         \x20       active_rules: {},\n\
         \x20       conditional_rules: {},\n\
         \x20       categories: {},\n\
         \x20   }}\n\
         }}\n\n",
        rules.len(),
        total_rules,
        conditional_rules,
        by_category.len()
    ));

    output.push_str(
        "/// Statistics about the generated rule set.\n\
         #[derive(Debug, Clone)]\n\
         #[allow(unused)]\n\
         pub(crate) struct GeneratedRuleStats {\n\
         \x20   pub total_rra_files: usize,\n\
         \x20   pub active_rules: usize,\n\
         \x20   pub conditional_rules: usize,\n\
         \x20   pub categories: usize,\n\
         }\n",
    );

    output
}
