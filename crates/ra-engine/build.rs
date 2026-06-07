//! Build script that compiles .rra rule files into Rust rewrite rules.
//!
//! Scans the `rules/` directory for `.rra` files, extracts YAML frontmatter
//! and `## Implementation` code blocks containing `rw!()` / `rewrite!()`
//! macros, and generates `$OUT_DIR/generated_rules.rs`.
#![expect(
    clippy::unwrap_used,
    clippy::allow_attributes,
    clippy::option_option,
    clippy::too_many_lines,
    clippy::format_push_string,
    clippy::manual_strip,
    reason = "build script — pragmatic code generation"
)]

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

    // RFC 0091 P1: compile per-operator cost models from physical rules that
    // declare `costs_operator:` + a `## Cost Model` block into a dispatch
    // registry (generated_costs.rs, included by cost.rs).
    let costs_path = Path::new(&out_dir).join("generated_costs.rs");
    let costs_src = generate_cost_registry(&rules_dir);
    fs::File::create(&costs_path)
        .unwrap()
        .write_all(costs_src.as_bytes())
        .unwrap();
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
    entries.sort_by_key(std::fs::DirEntry::path);
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
    // Prefer a structured `## Rewrite` block (declarative lhs/rhs/when data)
    // over a legacy embedded `rewrite!` code block. This lets a `.rra` carry
    // the rule as structured data rather than Rust source (RFC 0090). During
    // migration either form is accepted.
    let code_blocks = {
        let structured = extract_structured_rewrites(body, &id);
        if structured.is_empty() {
            extract_implementation_code(body)
        } else {
            structured
        }
    };

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

/// Extract `benefit_range` from frontmatter.
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

/// Extract a structured `## Rewrite` block and synthesize the equivalent
/// `rewrite!` macro source, which then flows through the same validation and
/// code-generation path as a legacy embedded block.
///
/// The structured form carries the rule as data, not Rust:
///
/// ```text
/// ## Rewrite
///
/// lhs: (filter ?pred (join inner ?cond ?left ?right))
/// rhs: (join inner ?cond (filter ?pred ?left) ?right)
/// when: references_only("?pred", "?left")     # optional, egg condition syntax
/// ```
///
/// `lhs`/`rhs` are S-expression patterns over `RelLang`; `when` (optional) is a
/// single named condition from `crate::conditions`. The synthesized macro is
/// named by the file's frontmatter `id`. Returns an empty vec when the file has
/// no `## Rewrite` section (callers then fall back to the legacy extractor).
fn extract_structured_rewrites(body: &str, id: &str) -> Vec<String> {
    let mut lhs: Option<String> = None;
    let mut rhs: Option<String> = None;
    let mut apply: Option<String> = None;
    let mut whens: Vec<String> = Vec::new();
    let mut in_section = false;

    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            if trimmed == "## Rewrite" {
                in_section = true;
                continue;
            } else if in_section {
                break; // left the section
            }
        }
        if !in_section {
            continue;
        }
        // Ignore code fences / comments / blanks inside the section.
        let line_no_comment = trimmed.split_once('#').map_or(trimmed, |(c, _)| c.trim_end());
        if let Some(v) = line_no_comment.strip_prefix("lhs:") {
            lhs = Some(v.trim().to_string());
        } else if let Some(v) = line_no_comment.strip_prefix("rhs:") {
            rhs = Some(v.trim().to_string());
        } else if let Some(v) = line_no_comment.strip_prefix("apply:") {
            let a = v.trim();
            if !a.is_empty() {
                apply = Some(a.to_string());
            }
        } else if let Some(v) = line_no_comment.strip_prefix("when:") {
            let w = v.trim();
            if !w.is_empty() {
                // A single `when:` may carry several conditions separated by
                // `;` (each becomes its own egg `if` clause); multiple `when:`
                // lines also accumulate.
                for part in w.split(';') {
                    let p = part.trim();
                    if !p.is_empty() {
                        whens.push(p.to_string());
                    }
                }
            }
        }
    }

    let mut cond = String::new();
    for w in &whens {
        cond.push_str("\n    if ");
        cond.push_str(w);
    }
    match (lhs, rhs, apply) {
        // Computed RHS via a named applier (apply: takes precedence over rhs:).
        (Some(l), _, Some(a)) if !l.is_empty() && !a.is_empty() => {
            vec![format!("rewrite!(\"{id}\";\n    \"{l}\" => {{ {a} }}{cond}\n)")]
        }
        // Static pattern RHS.
        (Some(l), Some(r), None) if !l.is_empty() && !r.is_empty() => {
            vec![format!("rewrite!(\"{id}\";\n    \"{l}\" =>\n    \"{r}\"{cond}\n)")]
        }
        _ => Vec::new(),
    }
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
// Conditions implemented in `crates/ra-engine/src/conditions.rs` that the
// build script can pass through unchanged. Adding a new entry here lets
// any `.rra` rule whose only blocker is one of these conditions compile
// into the active rewrite set instead of being commented out as TODO.
const KNOWN_CONDITIONS: &[&str] = &[
    "single_reference",
    "references_only",
    "pred_references_only",
    "predicate_references_only",
    "is_deterministic",
    "is_constant",
    "not_nullable",
    "not_zero",
    "is_canonical_scan",
    "is_uncorrelated",
    "is_xml_function_filter",
    "is_ungrouped_count_star",
    "is_not_const_bool",
    "is_bson_operator_filter",
    "is_json_field_predicate",
    "is_equi_join",
    "references_subset",
];

// Strip `if is_database("X")` lines from a rewrite block. The
// `databases:` YAML frontmatter already scopes the rule to specific
// engines, so the runtime check is redundant. Returns the cleaned
// rewrite source.
fn strip_is_database_condition(rewrite_str: &str) -> String {
    rewrite_str
        .lines()
        .filter(|line| {
            let t = line.trim_start();
            !t.starts_with("if is_database(")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Extract every `if <ident>(...)` condition name from a rewrite block.
/// Returns the set of condition function names mentioned.
///
/// Returns `None` if the rewrite contains a compound `if` expression
/// (e.g. `if a(...) && !b(...)`) that egg's `rewrite!` macro cannot
/// parse — those rules are inherently malformed and we can't extract
/// a clean set of names from them.
fn extract_condition_names(rewrite_str: &str) -> Option<Vec<String>> {
    let mut names = Vec::new();
    let Some(arrow_pos) = rewrite_str.find("=>") else {
        return Some(names);
    };
    let after_arrow = &rewrite_str[arrow_pos..];

    // Find each line that starts an `if` block. Lines that continue an
    // `if` (e.g. `&& other(...)` on the next line) get joined into the
    // current condition body.
    let mut current: Option<String> = None;
    let mut conditions: Vec<String> = Vec::new();
    for line in after_arrow.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("if ") {
            if let Some(prev) = current.take() {
                conditions.push(prev);
            }
            current = Some(rest.to_owned());
        } else if !trimmed.is_empty()
            && (trimmed.starts_with("&&") || trimmed.starts_with("||"))
        {
            if let Some(c) = current.as_mut() {
                c.push(' ');
                c.push_str(trimmed);
            }
        }
    }
    if let Some(prev) = current.take() {
        conditions.push(prev);
    }

    for body in &conditions {
        // Reject compound expressions — egg's `if` clause expects a
        // single Condition expression. Boolean operators between two
        // conditions can't be lowered without a real combinator
        // implementation.
        if body.contains("&&") || body.contains("||") || body.trim_start().starts_with('!') {
            return None;
        }
        // First identifier up to '(' is the condition name.
        if let Some(paren) = body.find('(') {
            let name = body[..paren].trim();
            if !name.is_empty() {
                names.push(name.to_owned());
            }
        }
    }
    Some(names)
}

/// Returns `true` if the rewrite has any condition or applier that the
/// build script can't currently materialise. Used to decide whether to
/// emit the rule as active or comment it out.
///
/// A condition is considered handleable if its name appears in
/// [`KNOWN_CONDITIONS`]; the corresponding constructor function is
/// imported by the wrapper module in `rewrite.rs`. `is_database(...)`
/// gets special treatment via [`strip_is_database_condition`] before
/// this check runs, so it never reaches here.
fn has_condition_or_applier(rewrite_str: &str) -> bool {
    let Some(names) = extract_condition_names(rewrite_str) else {
        // Compound `if` expression that egg can't parse → block.
        return true;
    };
    if names.iter().any(|n| !KNOWN_CONDITIONS.contains(&n.as_str())) {
        return true;
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
                .is_some_and(char::is_uppercase)
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
                    .is_some_and(char::is_uppercase)
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

/// All valid operators in `RelLang` with their expected arity.
/// Arity of 0 means leaf (no children), None means variadic (Box<[Id]>).
/// From egraph/lang.rs `define_language`! macro.
const RELLANG_OPERATORS: &[(&str, Option<usize>)] = &[
    // Relational operators
    ("scan", Some(1)), ("scan-alias", Some(2)), ("filter", Some(2)),
    ("project", Some(2)), ("join", Some(4)), ("aggregate", Some(3)),
    // Physical join variants (RFC 0090 Phase 3)
    ("hash-join", Some(4)), ("merge-join", Some(4)), ("nest-loop", Some(4)),
    ("index-nest-loop", Some(4)),
    ("index-scan-choice", Some(2)),
    ("sort", Some(2)), ("incremental-sort", Some(3)), ("limit", Some(3)),
    ("union", Some(3)), ("intersect", Some(3)), ("except", Some(3)),
    ("recursive-cte", Some(4)), ("cte", Some(3)), ("window", Some(2)),
    ("distinct-rel", Some(1)), ("values", None), ("values-row", None),
    // Metadata shortcut
    ("metadata-lookup", Some(2)), ("row-count", Some(0)),
    // Index operators
    ("index-scan", Some(2)), ("index-only-scan", Some(4)), ("mv-scan", Some(4)),
    // Bitmap operators
    ("bitmap-index-scan", Some(3)), ("bitmap-and", None), ("bitmap-or", None),
    ("bitmap-heap-scan", Some(3)),
    // Window function expression
    ("window-expr", Some(6)), ("window-fn", Some(1)), ("window-frame", Some(3)),
    ("frame-rows", Some(0)), ("frame-range", Some(0)), ("frame-groups", Some(0)),
    ("frame-unbounded-preceding", Some(0)), ("frame-preceding", Some(1)),
    ("frame-current-row", Some(0)), ("frame-following", Some(1)),
    ("frame-unbounded-following", Some(0)),
    // Join types
    ("inner", Some(0)), ("left-outer", Some(0)), ("right-outer", Some(0)),
    ("full-outer", Some(0)), ("cross", Some(0)), ("semi", Some(0)), ("anti", Some(0)),
    // Boolean flags
    ("true", Some(0)), ("false", Some(0)),
    // Scalar expressions
    ("col", Some(1)), ("qcol", Some(2)), ("const-null", Some(0)),
    ("const-bool", Some(1)), ("const-int", Some(1)), ("const-float", Some(1)),
    ("const-str", Some(1)),
    // Binary operators
    ("add", Some(2)), ("sub", Some(2)), ("mul", Some(2)), ("div", Some(2)),
    ("mod", Some(2)), ("eq", Some(2)), ("ne", Some(2)), ("lt", Some(2)),
    ("le", Some(2)), ("gt", Some(2)), ("ge", Some(2)), ("and", Some(2)),
    ("or", Some(2)), ("concat", Some(2)), ("json-access", Some(2)),
    // Unary operators
    ("not", Some(1)), ("is-null", Some(1)), ("is-not-null", Some(1)), ("neg", Some(1)),
    // Function call
    ("func", None),
    // Aggregate functions
    ("count", Some(1)), ("sum", Some(1)), ("avg", Some(1)),
    ("min", Some(1)), ("max", Some(1)),
    // Lists
    ("list", None), ("nil", Some(0)),
    // Projection column
    ("proj-col", Some(1)), ("proj-alias", Some(2)),
    // Sort keys
    ("sort-key", Some(3)), ("asc", Some(0)), ("desc", Some(0)),
    ("nulls-first", Some(0)), ("nulls-last", Some(0)),
    // Aggregate expression
    ("agg-expr", Some(3)), ("distinct", Some(0)), ("all", Some(0)),
    // Vector search operators
    ("vector-distance", Some(3)), ("vector-knn", Some(4)), ("vector-range-scan", Some(5)),
    // Full-text search operators
    ("fts-match", Some(4)), ("fts-rank", Some(3)), ("fts-index-scan", Some(3)),
    ("fts-ranked-scan", Some(5)), ("fts-skip-list-and", Some(3)),
    // Hybrid search operators
    ("hybrid-score", Some(5)), ("hybrid-scan", Some(6)),
    // Type casting
    ("cast", Some(2)),
];

/// Normalize operator aliases to canonical `RelLang` forms.
///
/// - `union_all` / `union-all` → `union true` (union with ALL flag)
fn normalize_pattern(pattern: &str) -> String {
    let mut result = pattern.to_string();
    // Normalize union_all and union-all to (union true ...)
    result = result.replace("(union_all ", "(union true ");
    result = result.replace("(union-all ", "(union true ");
    result
}

/// Look up an operator in the whitelist. Returns None if not found.
fn lookup_operator(name: &str) -> Option<Option<usize>> {
    RELLANG_OPERATORS
        .iter()
        .find(|(op, _)| *op == name)
        .map(|(_, arity)| *arity)
}

/// Check if a rewrite pattern string contains operators not in `RelLang`
/// or operators used with incorrect arity.
///
/// Parses `(op_name child1 child2 ...)` tokens within string literals
/// and validates against the `RELLANG_OPERATORS` whitelist.
fn contains_unknown_operators(code: &str) -> bool {
    if is_malformed_rule_pair(code) {
        return true;
    }
    // Extract operator usages from S-expression patterns in string literals.
    // We parse each `(op_name ...)` to check both name and arity.
    let chars: Vec<char> = code.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '"' {
            i += 1;
            // Parse inside this string literal
            let sexp_start = i;
            while i < chars.len() && chars[i] != '"' {
                i += 1;
            }
            let sexp: String = chars[sexp_start..i].iter().collect();
            if check_sexp_invalid(&sexp) {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// Check a single S-expression string for invalid operators or arity mismatches.
fn check_sexp_invalid(sexp: &str) -> bool {
    let chars: Vec<char> = sexp.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '(' {
            i += 1;
            // Skip whitespace
            while i < chars.len() && chars[i] == ' ' {
                i += 1;
            }
            // Extract operator name
            let start = i;
            while i < chars.len() && chars[i] != ' ' && chars[i] != ')' {
                i += 1;
            }
            if start < i {
                let op_name: String = chars[start..i].iter().collect();
                // Reject patterns with a metavariable in operator
                // position — egg's `rewrite!` macro only accepts
                // concrete operator names as the head of an
                // s-expression, never `?var`. Such patterns parse
                // syntactically but fail at runtime when the rule is
                // constructed (`UnexpectedVar` error). Two .rra rules
                // (push-func-filter-to-left/right) tripped this and
                // panicked `all_generated_rules()`, dropping the
                // entire generated batch via catch_unwind. We now
                // reject them at build time so the whole rule set
                // can load.
                if op_name.starts_with('?') {
                    return true;
                }
                if op_name.parse::<f64>().is_ok() || op_name.is_empty() {
                    continue;
                }
                match lookup_operator(&op_name) {
                    None => return true, // Unknown operator
                    Some(Some(expected_arity)) => {
                        // Count children (top-level items before matching close paren)
                        let child_count = count_children_at(chars.as_slice(), i);
                        if child_count != expected_arity {
                            return true; // Arity mismatch
                        }
                    }
                    Some(None) => {} // Variadic, any arity is fine
                }
            }
        } else {
            i += 1;
        }
    }
    false
}

/// Build-time gate that rejects empty / no-op / unbound-metavar rules.
///
/// `code` is the body of a `rewrite!()` invocation, including the
/// surrounding macro call. We extract the two adjacent string
/// literals (the LHS pattern and the RHS pattern) and check three
/// pathologies that egg either silently accepts or panics on at
/// rule-construction time:
///
/// 1. Either pattern empty (egg parser would reject; better here).
/// 2. LHS == RHS after whitespace normalisation — a rewrite-to-self
///    is a no-op and adds load without ever firing usefully.
/// 3. RHS references a `?metavar` that doesn't appear in LHS — egg
///    raises `UnexpectedVar` at runtime when the rule is built,
///    which (pre-audit-Item-6) panicked the whole batch.
///
/// Returns true when the rule is malformed and should be rejected.
fn is_malformed_rule_pair(code: &str) -> bool {
    let strings = extract_string_literals(code);
    // The `rewrite!()` macro signature is
    // `rewrite!("name"; "lhs" => "rhs")` — strings[0] is the rule
    // name, strings[1] is the LHS pattern, strings[2] is the RHS.
    // Check the LHS/RHS pair only; the name is just an identifier.
    let Some((lhs, rhs)) = strings.get(1).zip(strings.get(2)) else {
        // Couldn't find two patterns — leave the existing checks to
        // do their work and treat this as not-malformed.
        return false;
    };

    // Pathology 1: empty pattern.
    if lhs.trim().is_empty() || rhs.trim().is_empty() {
        println!(
            "cargo:warning=rejecting rule with empty pattern: \
             lhs={lhs:?} rhs={rhs:?}"
        );
        return true;
    }

    // Pathology 2: LHS == RHS modulo whitespace.
    let norm = |s: &str| {
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    if norm(lhs) == norm(rhs) {
        println!(
            "cargo:warning=rejecting no-op rule (LHS==RHS): \"{}\"",
            norm(lhs)
        );
        return true;
    }

    // Pathology 3: RHS metavar not bound on LHS.
    let lhs_vars = collect_metavars(lhs);
    let rhs_vars = collect_metavars(rhs);
    let unbound: Vec<_> = rhs_vars.iter().filter(|v| !lhs_vars.contains(*v)).collect();
    if !unbound.is_empty() {
        println!(
            "cargo:warning=rejecting rule with unbound RHS metavars: \
             rhs={rhs:?} unbound={unbound:?}"
        );
        return true;
    }
    false
}

/// Extract every double-quoted string literal in `code`. Naïve about
/// escaping, but sufficient for the rule corpus where `"` does not
/// appear inside a pattern.
fn extract_string_literals(code: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut iter = code.chars().peekable();
    while let Some(c) = iter.next() {
        if c == '"' {
            let mut buf = String::new();
            for ch in iter.by_ref() {
                if ch == '"' {
                    break;
                }
                buf.push(ch);
            }
            out.push(buf);
        }
    }
    out
}

/// Collect `?metavar` tokens from a pattern string. Returns names
/// without the leading `?`.
fn collect_metavars(s: &str) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '?' {
            i += 1;
            let start = i;
            while i < chars.len()
                && (chars[i].is_alphanumeric() || chars[i] == '_')
            {
                i += 1;
            }
            if start < i {
                out.insert(chars[start..i].iter().collect::<String>());
            }
        } else {
            i += 1;
        }
    }
    out
}

/// Count the number of top-level children starting from position `pos`
/// until the matching closing paren at depth 0.
fn count_children_at(chars: &[char], start: usize) -> usize {
    let mut count = 0;
    let mut i = start;
    let mut depth = 0;
    let mut in_token = false;

    while i < chars.len() {
        match chars[i] {
            ')' if depth == 0 => break,
            '(' => {
                if depth == 0 && !in_token {
                    count += 1;
                }
                depth += 1;
                in_token = false;
            }
            ')' => {
                depth -= 1;
                in_token = false;
            }
            ' ' | '\t' | '\n' | '\r' => {
                in_token = false;
            }
            _ => {
                if depth == 0 && !in_token {
                    count += 1;
                    in_token = true;
                } else if depth == 0 {
                    // still in token
                } else {
                    in_token = false;
                }
            }
        }
        i += 1;
    }
    count
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
            // Strip `if is_database("X")` lines: the YAML `databases:`
            // frontmatter already scopes the rule, so the runtime check
            // is redundant. Without this, every database-specific rule
            // is force-commented out even when no other blocker exists.
            rewrite_str = strip_is_database_condition(&rewrite_str);
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
            } else {
                // Apply operator normalization (union-all → union true, etc.)
                let normalized = normalize_pattern(&rewrite_str);

                // Validate that all operators exist in RelLang
                if contains_unknown_operators(&normalized) {
                    rewrites.push(ExtractedRule::Commented(format!(
                        "// TODO: contains operators not in RelLang\n        // {}",
                        normalized
                            .lines()
                            .collect::<Vec<_>>()
                            .join("\n        // ")
                    )));
                } else if normalized.contains("<=>") {
                    // Bidirectional rewrite — produces Vec<Rewrite>
                    rewrites.push(ExtractedRule::Bidirectional(normalized));
                } else {
                    rewrites.push(ExtractedRule::Single(normalized));
                }
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
/// e.g. "logical/predicate-pushdown" -> "`logical_predicate_pushdown`"
fn category_to_fn_name(category: &str) -> String {
    category
        .replace(['/', '-', ' '], "_")
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
                                "        {rule_comment}\n        {code},"
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
                                "        {rule_comment}\n        {code}"
                            ));
                            conditional_rules += 1;
                        }
                    }
                }
            }
        }

        output.push_str(&format!(
            "/// Generated rules for category: {category}\n"
        ));
        output.push_str(&format!(
            "/// Source: {} .rra files\n",
            cat_rules.len()
        ));
        output.push_str("#[allow(unused, clippy::too_many_lines)]\n");
        output.push_str(&format!(
            "pub(crate) fn {fn_name}() -> Vec<Rewrite<RelLang, RelAnalysis>> {{\n"
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
                    "    // Bidirectional: {id}\n    rules.extend({code});\n"
                ));
            }
            output.push_str("    rules\n");
        }

        output.push_str("}\n\n");
    }

    // Generate master function that collects all categories
    output.push_str(&format!(
        "/// All generated rules from .rra files.\n\
         ///\n\
         /// Total: {total_rules} active rules, {conditional_rules} conditional (awaiting condition functions).\n\
         ///\n\
         /// Each category's rules are wrapped in `catch_unwind` so a\n\
         /// single malformed rule (e.g. a pattern with a metavariable\n\
         /// in operator position that survives the build-time\n\
         /// validator) only drops its own category instead of the\n\
         /// entire generated set. The build script's\n\
         /// `check_sexp_invalid` rejects most such patterns; the\n\
         /// `catch_unwind` here is defensive belt-and-suspenders.\n\
         #[allow(unused, clippy::too_many_lines)]\n\
         pub(crate) fn all_generated_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {{\n\
         "
    ));
    output.push_str(&format!(
        "    let mut rules = Vec::with_capacity({total_rules});\n"
    ));
    for fn_name in &category_fns {
        output.push_str(&format!(
            "    if let Ok(category) = std::panic::catch_unwind(\
             std::panic::AssertUnwindSafe({fn_name})) {{\n\
             \x20       rules.extend(category);\n\
             \x20   }}\n"
        ));
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

// ---------------------------------------------------------------------------
// RFC 0091 P1: per-operator cost-model compilation
// ---------------------------------------------------------------------------

/// Scan `.rra` files for `costs_operator:` frontmatter + a `## Cost Model`
/// Rust block, and emit `generated_costs.rs`: one `fn <id>_cost(ctx) -> f64`
/// per declaring rule plus an `OPERATOR_COST_FNS` registry keyed by operator.
fn generate_cost_registry(dir: &Path) -> String {
    let mut entries: Vec<(String, String, String)> = Vec::new(); // (operator, fn_name, body)
    collect_cost_models(dir, &mut entries);
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut out = String::new();
    out.push_str("// @generated by build.rs from .rra `## Cost Model` blocks (RFC 0091).\n");
    out.push_str("// Each fn is the per-operator cost body; the registry maps a RelLang\n");
    out.push_str("// physical-operator name to its cost function.\n\n");
    for (_op, fn_name, body) in &entries {
        out.push_str(&format!(
            "#[allow(clippy::all, clippy::pedantic, unused_variables)]\n\
             fn {fn_name}(ctx: &OperatorCostCtx) -> f64 {{\n    {body}\n}}\n\n"
        ));
    }
    out.push_str("/// Operator name -> cost function, compiled from the rule files.\n");
    out.push_str("pub(crate) static OPERATOR_COST_FNS: &[(&str, OperatorCostFn)] = &[\n");
    for (op, fn_name, _body) in &entries {
        out.push_str(&format!("    ({op:?}, {fn_name}),\n"));
    }
    out.push_str("];\n");
    out
}

fn collect_cost_models(dir: &Path, out: &mut Vec<(String, String, String)>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_cost_models(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rra") {
            if let Some((op, id, body)) = parse_cost_model(&path) {
                let fn_name = format!("cost_{}", id.replace(['-', '.', '/'], "_"));
                out.push((op, fn_name, body));
            }
        }
    }
}

/// Parse a `.rra` for `costs_operator:` + a `## Cost Model` rust block.
fn parse_cost_model(path: &Path) -> Option<(String, String, String)> {
    let content = fs::read_to_string(path).ok()?;
    let parts: Vec<&str> = content.splitn(3, "---").collect();
    if parts.len() < 3 {
        return None;
    }
    let frontmatter = parts[1].trim();
    let operator = extract_yaml_string(frontmatter, "costs_operator")?;
    let id = extract_yaml_string(frontmatter, "id")?;
    let body = extract_section_rust(parts[2], "## Cost Model")?;
    Some((operator, id, body))
}

/// Extract the first fenced Rust code block under the given `## Section`.
fn extract_section_rust(body: &str, section: &str) -> Option<String> {
    let mut in_section = false;
    let mut in_code = false;
    let mut block = String::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            if trimmed == section {
                in_section = true;
            } else if in_section {
                break;
            }
            continue;
        }
        if !in_section {
            continue;
        }
        if trimmed.starts_with("```") && !in_code {
            let lang = trimmed[3..].trim();
            if lang == "rust" || lang.is_empty() {
                in_code = true;
                block.clear();
            }
        } else if trimmed == "```" && in_code {
            let b = block.trim().to_string();
            if !b.is_empty() {
                return Some(b);
            }
            in_code = false;
        } else if in_code {
            block.push_str(line);
            block.push('\n');
        }
    }
    None
}
