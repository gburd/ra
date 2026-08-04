// Shared malformed-rewrite-pattern detection for `.rra` rule files.
//
// This module is the single source of truth for the three "pathology"
// checks that reject a `rewrite!("name"; "lhs" => "rhs")` pattern:
//
// 1. an empty LHS or RHS pattern,
// 2. an LHS equal to the RHS modulo whitespace (a no-op rule),
// 3. an RHS metavariable (`?name`) not bound on the LHS.
//
// It is compiled both as a normal crate module (`ra_engine::rule_lint`,
// used by the `ra rules lint` CLI command) **and** `include!`-ed directly
// into `crates/ra-engine/build.rs`, which cannot depend on the crate it
// builds. To keep the second use working the module is **std-only**: no
// imports from `crate::` or external crates, and the unit tests are behind
// `#[cfg(test)]` so they never reach the build script's compilation unit.

use std::collections::HashSet;

/// Check a single `rewrite!()` invocation source for the three malformed
/// pattern pathologies. Returns `Some(reason)` when malformed (the reason
/// string mirrors the historical build-script `cargo:warning` text), or
/// `None` when the LHS/RHS pair is well-formed (or no pair was found).
///
/// `code` is the body of a `rewrite!(...)` invocation, including the macro
/// call. The macro signature is `rewrite!("name"; "lhs" => "rhs")`, so of the
/// double-quoted literals we take `[1]` (LHS) and `[2]` (RHS); `[0]` is the
/// rule name.
#[must_use]
pub fn rewrite_pair_pathology(code: &str) -> Option<String> {
    let strings = extract_string_literals(code);
    // Need at least a name + LHS + RHS. If we can't find two patterns, treat
    // as not-malformed (a computed-RHS `=> { applier }` form has only the LHS
    // pattern as a string literal and is out of scope for this check).
    let (lhs, rhs) = (strings.get(1)?, strings.get(2)?);

    // Pathology 1: empty pattern.
    if lhs.trim().is_empty() || rhs.trim().is_empty() {
        return Some(format!("empty pattern: lhs={lhs:?} rhs={rhs:?}"));
    }

    // Pathology 2: LHS == RHS modulo whitespace.
    let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    if norm(lhs) == norm(rhs) {
        return Some(format!("no-op rule (LHS==RHS): \"{}\"", norm(lhs)));
    }

    // Pathology 3: RHS metavar not bound on LHS.
    let lhs_vars = collect_metavars(lhs);
    let rhs_vars = collect_metavars(rhs);
    let mut unbound: Vec<&String> = rhs_vars.iter().filter(|v| !lhs_vars.contains(*v)).collect();
    if !unbound.is_empty() {
        unbound.sort();
        return Some(format!(
            "unbound RHS metavars: rhs={rhs:?} unbound={unbound:?}"
        ));
    }
    None
}

/// Extract every double-quoted string literal in `code`. Naïve about
/// escaping, but sufficient for the rule corpus where `"` never appears
/// inside a pattern.
#[must_use]
pub fn extract_string_literals(code: &str) -> Vec<String> {
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

/// Collect `?metavar` tokens from a pattern string. Returns the names
/// without the leading `?`.
#[must_use]
pub fn collect_metavars(s: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '?' {
            i += 1;
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
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

/// Extract every `rw!(...)` / `rewrite!(...)` invocation from a `.rra`
/// file *body*, returning each invocation's source (normalized to
/// `rewrite!(`). This is the same view `build.rs` takes: it prefers a
/// structured `## Rewrite` block (synthesized to `rewrite!` source) and
/// otherwise reads embedded macro calls from the `## Implementation`
/// section, then splits a multi-rule code block into individual
/// invocations by balanced parens.
///
/// The returned strings are exactly what [`rewrite_pair_pathology`]
/// expects. Used by the `ra rules lint` CLI so it reproduces the build
/// script's extraction rather than re-deriving it.
#[must_use]
pub fn extract_rewrite_invocations(body: &str, id: &str) -> Vec<String> {
    let blocks = {
        let structured = synthesize_structured_rewrites(body, id);
        if structured.is_empty() {
            extract_implementation_blocks(body)
        } else {
            structured
        }
    };
    let mut invocations = Vec::new();
    for block in &blocks {
        split_rewrite_invocations(block, &mut invocations);
    }
    invocations
}

/// Split a code block containing one or more `rw!(...)` / `rewrite!(...)`
/// macro calls into individual invocation strings (each starting with
/// `rewrite!(`), matching balanced parentheses. Mirrors the extraction in
/// `build.rs::normalize_rewrite_code`.
fn split_rewrite_invocations(block: &str, out: &mut Vec<String>) {
    let normalized = block.replace("rw!(", "rewrite!(");
    let marker = "rewrite!(";
    let mut search_from = 0;
    while let Some(start) = normalized[search_from..].find(marker) {
        let abs_start = search_from + start;
        let after_marker = abs_start + marker.len();
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
            out.push(normalized[abs_start..end_pos].trim().to_string());
            search_from = end_pos;
        } else {
            // Unbalanced parens — stop scanning this block.
            break;
        }
    }
}

/// Extract Rust code blocks from the `## Implementation` section that
/// contain rewrite macros. Mirrors `build.rs::extract_implementation_code`.
fn extract_implementation_blocks(body: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut in_impl_section = false;
    let mut in_code_block = false;
    let mut current_block = String::new();

    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            if trimmed == "## Implementation" {
                in_impl_section = true;
            } else if in_impl_section {
                break;
            }
        }
        if !in_impl_section {
            continue;
        }
        if trimmed.starts_with("```") && !in_code_block {
            let lang = trimmed[3..].trim();
            if lang == "rust" || lang.is_empty() {
                in_code_block = true;
                current_block.clear();
            }
        } else if trimmed == "```" && in_code_block {
            in_code_block = false;
            let block = current_block.trim().to_string();
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

/// Synthesize `rewrite!` source from a structured `## Rewrite` block.
/// Mirrors `build.rs::extract_structured_rewrites`. Returns an empty vec
/// when the file has no `## Rewrite` section.
fn synthesize_structured_rewrites(body: &str, id: &str) -> Vec<String> {
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
                break;
            }
        }
        if !in_section {
            continue;
        }
        let line_no_comment = trimmed
            .split_once('#')
            .map_or(trimmed, |(c, _)| c.trim_end());
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
        (Some(l), _, Some(a)) if !l.is_empty() && !a.is_empty() => {
            vec![format!(
                "rewrite!(\"{id}\";\n    \"{l}\" => {{ {a} }}{cond}\n)"
            )]
        }
        (Some(l), Some(r), None) if !l.is_empty() && !r.is_empty() => {
            vec![format!(
                "rewrite!(\"{id}\";\n    \"{l}\" =>\n    \"{r}\"{cond}\n)"
            )]
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_rule_is_not_malformed() {
        let code = r#"rewrite!("push-filter"; "(filter ?p (scan ?t))" => "(scan ?t)")"#;
        assert_eq!(rewrite_pair_pathology(code), None);
    }

    #[test]
    fn empty_pattern_is_malformed() {
        let code = r#"rewrite!("bad"; "" => "(scan ?t)")"#;
        let r = rewrite_pair_pathology(code).expect("should be malformed");
        assert!(r.contains("empty pattern"), "got: {r}");
    }

    #[test]
    fn noop_rule_is_malformed() {
        let code = r#"rewrite!("noop"; "(filter ?p ?x)" => "(filter  ?p   ?x)")"#;
        let r = rewrite_pair_pathology(code).expect("should be malformed");
        assert!(r.contains("no-op"), "got: {r}");
    }

    #[test]
    fn unbound_rhs_metavar_is_malformed() {
        let code = r#"rewrite!("unbound"; "(filter ?p ?x)" => "(filter ?p ?y)")"#;
        let r = rewrite_pair_pathology(code).expect("should be malformed");
        assert!(r.contains("unbound RHS metavars"), "got: {r}");
        assert!(r.contains("\"y\""), "got: {r}");
    }

    #[test]
    fn missing_pair_is_not_malformed() {
        // Only a name literal (computed-RHS applier form) — out of scope.
        let code = r#"rewrite!("x"; "(scan ?t)" => { MyApplier { } })"#;
        assert_eq!(rewrite_pair_pathology(code), None);
    }

    #[test]
    fn extract_rewrite_invocations_from_implementation() {
        let body = "\
## Implementation

```rust
use egg::*;
rw!(\"a\"; \"(filter ?p ?x)\" => \"(scan ?x)\");
rw!(\"b\"; \"(project ?c ?x)\" => \"(project ?c ?y)\");
```
";
        let invs = extract_rewrite_invocations(body, "ignored");
        assert_eq!(invs.len(), 2);
        assert!(invs[0].starts_with("rewrite!("));
        // Second one has an unbound metavar.
        assert!(rewrite_pair_pathology(&invs[1]).is_some());
        assert!(rewrite_pair_pathology(&invs[0]).is_none());
    }

    #[test]
    fn extract_rewrite_invocations_from_structured_rewrite() {
        let body = "\
## Rewrite

lhs: (filter ?p ?x)
rhs: (filter ?p ?x)
";
        let invs = extract_rewrite_invocations(body, "noop-id");
        assert_eq!(invs.len(), 1);
        let r = rewrite_pair_pathology(&invs[0]).expect("no-op should be caught");
        assert!(r.contains("no-op"), "got: {r}");
    }
}
