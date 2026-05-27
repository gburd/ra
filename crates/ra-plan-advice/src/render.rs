//! AST → string renderer for plan advice.
//!
//! The output is a syntactically valid plan-advice string that
//! parses back to the same AST via [`crate::parse_advice`]. PG
//! does not have a direct AST renderer (it emits advice from
//! finished plans, not from re-printing parsed advice), so this
//! is net-new code; the syntax it emits matches `pgpa_parser.y`.
//!
//! # Round-trip property
//!
//! For any `Advice` value `a`, `parse_advice(&render_advice(&a)) ==
//! Ok(a)`. This is verified by the property test in `tests/`.

use std::fmt::Write as _;

use crate::ast::{
    Advice, AdviceItem, AdviceTag, AdviceTarget, AdviceTargetKind,
    IndexTarget, RelationIdentifier,
};

/// Render an [`Advice`] value as a plan-advice string.
///
/// Items are separated by spaces; targets within an item are
/// separated by spaces too. The renderer never emits trailing
/// whitespace and never emits a leading or trailing newline.
///
/// # Examples
///
/// ```
/// use ra_plan_advice::{
///     parse_advice, render_advice,
///     AdviceItem, AdviceTag, AdviceTarget, RelationIdentifier,
/// };
///
/// let advice = vec![AdviceItem {
///     tag: AdviceTag::SeqScan,
///     targets: vec![AdviceTarget::identifier(
///         RelationIdentifier::simple("t"),
///     )],
/// }];
/// assert_eq!(render_advice(&advice), "SEQ_SCAN(t)");
/// ```
#[must_use]
pub fn render_advice(advice: &Advice) -> String {
    let mut out = String::new();
    for (i, item) in advice.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        render_item(&mut out, item);
    }
    out
}

fn render_item(out: &mut String, item: &AdviceItem) {
    out.push_str(item.tag.as_keyword());
    out.push('(');
    let needs_index = item.tag.requires_index_argument();
    for (i, target) in item.targets.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        render_target(out, target, needs_index);
    }
    out.push(')');
}

fn render_target(out: &mut String, target: &AdviceTarget, needs_index: bool) {
    match target.kind {
        AdviceTargetKind::Identifier => {
            // Bare identifier targets always have Some(identifier);
            // emit a placeholder otherwise to avoid panicking on
            // ill-formed input. The parser never produces this
            // shape, so a missing identifier here is a caller bug.
            if let Some(rid) = &target.identifier {
                render_identifier(out, rid);
            }
            // INDEX_SCAN/INDEX_ONLY_SCAN always carries an index
            // target after each identifier; PG's grammar requires
            // it.
            if needs_index {
                if let Some(idx) = &target.index {
                    out.push(' ');
                    render_index_target(out, idx);
                }
            }
        }
        AdviceTargetKind::OrderedList => {
            out.push('(');
            for (i, child) in target.children.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                render_target(out, child, false);
            }
            out.push(')');
        }
        AdviceTargetKind::UnorderedList => {
            out.push('{');
            for (i, child) in target.children.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                render_target(out, child, false);
            }
            out.push('}');
        }
    }
}

fn render_identifier(out: &mut String, rid: &RelationIdentifier) {
    write_quoted(out, &rid.alias_name);
    if rid.occurrence != 1 {
        // The default occurrence is 1; PG omits `#1` in generated
        // advice but accepts it on input. We omit it on output too.
        let _ = write!(out, "#{}", rid.occurrence);
    }
    if let Some(part) = &rid.partition_name {
        out.push('/');
        if let Some(schema) = &rid.partition_schema {
            write_quoted(out, schema);
            out.push('.');
        }
        write_quoted(out, part);
    }
    if let Some(plan) = &rid.plan_name {
        out.push('@');
        write_quoted(out, plan);
    }
}

fn render_index_target(out: &mut String, idx: &IndexTarget) {
    if let Some(schema) = &idx.schema {
        write_quoted(out, schema);
        out.push('.');
    }
    write_quoted(out, &idx.name);
}

/// Write `s` as either a bare identifier or a double-quoted
/// identifier, choosing whichever produces a valid token. The
/// scanner accepts a bare identifier matching `[A-Za-z_\200-\377]
/// [A-Za-z0-9_$\200-\377]*`; anything else needs quoting.
fn write_quoted(out: &mut String, s: &str) {
    if needs_quoting(s) {
        out.push('"');
        for c in s.chars() {
            if c == '"' {
                out.push('"');
                out.push('"');
            } else {
                out.push(c);
            }
        }
        out.push('"');
    } else {
        out.push_str(s);
    }
}

fn needs_quoting(s: &str) -> bool {
    if s.is_empty() {
        // Empty identifiers are rejected by both parser and
        // renderer; the only safe stringification is `""`, but
        // PG itself also raises "zero-length delimited
        // identifier", so empty is fundamentally invalid.
        return true;
    }
    let mut bytes = s.bytes();
    // We checked s.is_empty() above, so .next() returns Some.
    let Some(first) = bytes.next() else { return true };
    if !(first.is_ascii_alphabetic() || first == b'_' || first >= 0x80) {
        return true;
    }
    for b in bytes {
        if !(b.is_ascii_alphanumeric()
            || b == b'_'
            || b == b'$'
            || b >= 0x80)
        {
            return true;
        }
    }
    // Tags must be quoted when used as identifiers, since the
    // scanner promotes them to TOK_TAG_* tokens.
    if AdviceTag::from_lowercase(s).is_some() {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::AdviceTarget;

    #[test]
    fn simple_seq_scan() {
        let advice = vec![AdviceItem {
            tag: AdviceTag::SeqScan,
            targets: vec![
                AdviceTarget::identifier(RelationIdentifier::simple("t")),
            ],
        }];
        assert_eq!(render_advice(&advice), "SEQ_SCAN(t)");
    }

    #[test]
    fn join_order_with_subliststs() {
        let advice = vec![AdviceItem {
            tag: AdviceTag::JoinOrder,
            targets: vec![
                AdviceTarget::identifier(RelationIdentifier::simple("a")),
                AdviceTarget::ordered(vec![
                    AdviceTarget::identifier(RelationIdentifier::simple("b")),
                    AdviceTarget::identifier(RelationIdentifier::simple("c")),
                ]),
                AdviceTarget::identifier(RelationIdentifier::simple("d")),
            ],
        }];
        assert_eq!(render_advice(&advice), "JOIN_ORDER(a (b c) d)");
    }

    #[test]
    fn join_order_with_unordered_sublist() {
        let advice = vec![AdviceItem {
            tag: AdviceTag::JoinOrder,
            targets: vec![
                AdviceTarget::identifier(RelationIdentifier::simple("a")),
                AdviceTarget::unordered(vec![
                    AdviceTarget::identifier(RelationIdentifier::simple("b")),
                    AdviceTarget::identifier(RelationIdentifier::simple("c")),
                ]),
            ],
        }];
        assert_eq!(render_advice(&advice), "JOIN_ORDER(a {b c})");
    }

    #[test]
    fn index_scan_includes_index_name() {
        let mut t = AdviceTarget::identifier(RelationIdentifier::simple("foo"));
        t.index = Some(IndexTarget {
            schema: None,
            name: "foo_a_idx".into(),
        });
        let advice = vec![AdviceItem {
            tag: AdviceTag::IndexScan,
            targets: vec![t],
        }];
        assert_eq!(render_advice(&advice), "INDEX_SCAN(foo foo_a_idx)");
    }

    #[test]
    fn index_scan_with_schema_qualified_index() {
        let mut t = AdviceTarget::identifier(RelationIdentifier::simple("foo"));
        t.index = Some(IndexTarget {
            schema: Some("public".into()),
            name: "foo_a_idx".into(),
        });
        let advice = vec![AdviceItem {
            tag: AdviceTag::IndexScan,
            targets: vec![t],
        }];
        assert_eq!(render_advice(&advice), "INDEX_SCAN(foo public.foo_a_idx)");
    }

    #[test]
    fn relation_identifier_with_all_components() {
        let rid = RelationIdentifier {
            alias_name: "tab".into(),
            occurrence: 2,
            partition_schema: Some("public".into()),
            partition_name: Some("tab_2025".into()),
            plan_name: Some("any_1".into()),
        };
        let advice = vec![AdviceItem {
            tag: AdviceTag::SeqScan,
            targets: vec![AdviceTarget::identifier(rid)],
        }];
        assert_eq!(
            render_advice(&advice),
            "SEQ_SCAN(tab#2/public.tab_2025@any_1)",
        );
    }

    #[test]
    fn occurrence_one_is_omitted() {
        let advice = vec![AdviceItem {
            tag: AdviceTag::SeqScan,
            targets: vec![AdviceTarget::identifier(RelationIdentifier {
                alias_name: "tab".into(),
                occurrence: 1,
                partition_schema: None,
                partition_name: None,
                plan_name: None,
            })],
        }];
        assert_eq!(render_advice(&advice), "SEQ_SCAN(tab)");
    }

    #[test]
    fn identifier_with_special_chars_gets_quoted() {
        let advice = vec![AdviceItem {
            tag: AdviceTag::SeqScan,
            targets: vec![AdviceTarget::identifier(
                RelationIdentifier::simple("has space"),
            )],
        }];
        assert_eq!(render_advice(&advice), r#"SEQ_SCAN("has space")"#);
    }

    #[test]
    fn identifier_starting_with_digit_gets_quoted() {
        let advice = vec![AdviceItem {
            tag: AdviceTag::SeqScan,
            targets: vec![AdviceTarget::identifier(
                RelationIdentifier::simple("2col"),
            )],
        }];
        assert_eq!(render_advice(&advice), r#"SEQ_SCAN("2col")"#);
    }

    #[test]
    fn embedded_double_quote_doubles() {
        let advice = vec![AdviceItem {
            tag: AdviceTag::SeqScan,
            targets: vec![AdviceTarget::identifier(
                RelationIdentifier::simple(r#"with"quote"#),
            )],
        }];
        assert_eq!(
            render_advice(&advice),
            r#"SEQ_SCAN("with""quote")"#,
        );
    }

    #[test]
    fn alias_named_like_a_tag_gets_quoted() {
        // An alias literally called "seq_scan" has to be quoted
        // because the scanner would otherwise promote it to
        // TOK_TAG_SIMPLE and cause an unexpected-token error.
        let advice = vec![AdviceItem {
            tag: AdviceTag::SeqScan,
            targets: vec![AdviceTarget::identifier(
                RelationIdentifier::simple("seq_scan"),
            )],
        }];
        assert_eq!(render_advice(&advice), r#"SEQ_SCAN("seq_scan")"#);
    }

    #[test]
    fn multiple_items_separated_by_spaces() {
        let advice = vec![
            AdviceItem {
                tag: AdviceTag::SeqScan,
                targets: vec![AdviceTarget::identifier(
                    RelationIdentifier::simple("a"),
                )],
            },
            AdviceItem {
                tag: AdviceTag::HashJoin,
                targets: vec![AdviceTarget::identifier(
                    RelationIdentifier::simple("b"),
                )],
            },
        ];
        assert_eq!(render_advice(&advice), "SEQ_SCAN(a) HASH_JOIN(b)");
    }
}
