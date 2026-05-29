#![expect(clippy::unwrap_used, reason = "test code; unwrap is the conventional shorthand for surfacing failures in tests")]
#![expect(clippy::panic, reason = "test code; panic is how we report a failed expectation")]
//! Parser conformance against the test cases in
//! `~/src/postgres/contrib/pg_plan_advice/sql/syntax.sql`.
//!
//! Each test case is sourced from a `SET pg_plan_advice.advice = '...'`
//! line in PG's regression suite. Cases under "Syntax errors"
//! must fail to parse; everything else must parse without error.

use ra_plan_advice::{
    parse_advice, AdviceTag, AdviceTargetKind,
};

// ────────────────────────────────────────────────────────────────
// Positive: should parse without error
// ────────────────────────────────────────────────────────────────

/// Empty advice strings, including ones with an empty target list,
/// are legal for tags other than `JOIN_ORDER`.
#[test]
fn empty_string_parses_to_empty_advice() {
    let out = parse_advice("").unwrap();
    assert!(out.is_empty());
}

#[test]
fn empty_target_list_legal_for_seq_scan() {
    let out = parse_advice("SEQ_SCAN()").unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].tag, AdviceTag::SeqScan);
    assert!(out[0].targets.is_empty());
}

#[test]
fn empty_target_list_legal_for_nested_loop_plain() {
    let out = parse_advice("NESTED_LOOP_PLAIN()").unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].tag, AdviceTag::NestedLoopPlain);
    assert!(out[0].targets.is_empty());
}

#[test]
fn empty_target_list_illegal_for_join_order() {
    let err = parse_advice("JOIN_ORDER()").unwrap_err();
    assert!(
        err.message.contains("JOIN_ORDER must have at least one target"),
        "{}",
        err.message
    );
}

#[test]
fn capitalization_variations_normalize() {
    // Mixed-case tag names downcase to the canonical form.
    for s in [
        "SEQ_SCAN(x)",
        "seq_scan(x)",
        "Seq_Scan(x)",
        "SEQ_scan(x)",
    ] {
        let out = parse_advice(s).unwrap_or_else(|e| panic!("{s}: {e}"));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].tag, AdviceTag::SeqScan);
    }
}

#[test]
fn whitespace_around_punctuation_ignored() {
    for s in [
        "SEQ_SCAN(x)",
        "SEQ_SCAN (x)",
        "SEQ_SCAN ( x )",
        "  SEQ_SCAN ( x / y . z )  ",
        "SEQ_SCAN\n(\n  x\n)\n",
    ] {
        parse_advice(s).unwrap_or_else(|e| panic!("{s}: {e}"));
    }
}

#[test]
fn relation_identifier_components_optional() {
    // alias-only, alias@plan, alias#occurrence, alias/partition
    for s in [
        "SEQ_SCAN(x)",
        "seq_scan(x@y)",
        "SEQ_scan(x#2)",
        "SEQ_SCAN(x/y)",
        "SEQ_SCAN(x/y.z)",
    ] {
        parse_advice(s).unwrap_or_else(|e| panic!("{s}: {e}"));
    }
}

#[test]
fn fully_specified_relation_identifier() {
    let out = parse_advice("SEQ_SCAN(x#2/y.z@t)").unwrap();
    let rid = out[0].targets[0].identifier.as_ref().unwrap();
    assert_eq!(rid.alias_name, "x");
    assert_eq!(rid.occurrence, 2);
    assert_eq!(rid.partition_schema.as_deref(), Some("y"));
    assert_eq!(rid.partition_name.as_deref(), Some("z"));
    assert_eq!(rid.plan_name.as_deref(), Some("t"));
}

#[test]
fn quoted_identifier_in_every_component() {
    let out = parse_advice(r##"SEQ_SCAN("x"#2/"y"."z"@"t")"##).unwrap();
    let rid = out[0].targets[0].identifier.as_ref().unwrap();
    assert_eq!(rid.alias_name, "x");
    assert_eq!(rid.occurrence, 2);
    assert_eq!(rid.partition_schema.as_deref(), Some("y"));
    assert_eq!(rid.partition_name.as_deref(), Some("z"));
    assert_eq!(rid.plan_name.as_deref(), Some("t"));
}

#[test]
fn comments_are_skipped() {
    for s in [
        "/**/",
        "HASH_JOIN(_)/***/",
        "/* comment */ HASH_JOIN(/*x*/y)",
        "/* comment */ HASH_JOIN(y/*x*/)", // PG syntax.sql line 53
    ] {
        parse_advice(s).unwrap_or_else(|e| panic!("{s}: {e}"));
    }
}

#[test]
fn nested_comments_unsupported_outer_terminates_first() {
    // From PG's syntax.sql:
    //   '/*/*/'  -- legal: outer ends at first '*/'
    let out = parse_advice("/*/*/").unwrap();
    assert!(out.is_empty());
}

#[test]
fn tag_keywords_legal_as_alias_names() {
    // From syntax.sql: "Tag keywords used as alias names work fine"
    for s in [
        "SEQ_SCAN(hash_join)",
        "SEQ_SCAN(seq_scan)",
        "SEQ_SCAN(gather)",
        "SEQ_SCAN(join_order)",
    ] {
        parse_advice(s).unwrap_or_else(|e| panic!("{s}: {e}"));
    }
}

#[test]
fn join_order_with_ordered_sublist() {
    let out = parse_advice("JOIN_ORDER(a (b c) d)").unwrap();
    assert_eq!(out[0].targets.len(), 3);
    assert_eq!(out[0].targets[0].kind, AdviceTargetKind::Identifier);
    assert_eq!(out[0].targets[1].kind, AdviceTargetKind::OrderedList);
    assert_eq!(out[0].targets[2].kind, AdviceTargetKind::Identifier);
    assert_eq!(out[0].targets[1].children.len(), 2);
}

#[test]
fn join_order_with_unordered_sublist() {
    let out = parse_advice("JOIN_ORDER(a {b c} d)").unwrap();
    assert_eq!(out[0].targets[1].kind, AdviceTargetKind::UnorderedList);
}

#[test]
fn nested_ordered_sublists_under_join_order() {
    let out = parse_advice("JOIN_ORDER(a (b (c d)))").unwrap();
    assert_eq!(out[0].targets[1].kind, AdviceTargetKind::OrderedList);
    assert_eq!(
        out[0].targets[1].children[1].kind,
        AdviceTargetKind::OrderedList
    );
}

// ────────────────────────────────────────────────────────────────
// Negative: should NOT parse
// ────────────────────────────────────────────────────────────────

#[test]
fn unknown_tag_rejected() {
    let err = parse_advice("SEQUENTIAL_SCAN(x)").unwrap_err();
    assert!(err.message.to_lowercase().contains("unrecognized"));
}

#[test]
fn missing_open_paren_rejected() {
    parse_advice("SEQ_SCAN").unwrap_err();
}

#[test]
fn unterminated_target_list_rejected() {
    parse_advice("SEQ_SCAN(").unwrap_err();
}

#[test]
fn unterminated_quoted_identifier_rejected() {
    parse_advice(r#"SEQ_SCAN(""#).unwrap_err();
}

#[test]
fn zero_length_quoted_identifier_rejected() {
    parse_advice(r#"SEQ_SCAN("")"#).unwrap_err();
}

#[test]
fn unterminated_target_list_after_identifier_rejected() {
    parse_advice(r#"SEQ_SCAN("a""#).unwrap_err();
}

#[test]
fn occurrence_without_number_rejected() {
    parse_advice("SEQ_SCAN(#").unwrap_err();
}

#[test]
fn paren_at_top_level_rejected() {
    parse_advice("()").unwrap_err();
}

#[test]
fn bare_integer_at_top_level_rejected() {
    parse_advice("123").unwrap_err();
}

#[test]
fn simple_tag_rejects_sublist() {
    // From syntax.sql: SEQ_SCAN doesn't allow sublists at all.
    parse_advice("SEQ_SCAN((x))").unwrap_err();
}

#[test]
fn generic_tag_rejects_doubly_nested_sublist() {
    // From syntax.sql: GATHER allows one level of sublist, no more.
    parse_advice("GATHER(((x)))").unwrap_err();
}

#[test]
fn unterminated_comment_rejected() {
    parse_advice("/*").unwrap_err();
}

#[test]
fn unterminated_comment_after_advice_rejected() {
    parse_advice(r#"JOIN_ORDER("fOO") /* oops"#).unwrap_err();
}

#[test]
fn nested_comment_rejected() {
    // From syntax.sql: nested comments unsupported.
    // '/*/* stuff */*/' -- the '*/' inside terminates the first
    // comment, leaving '*/' as bare junk.
    parse_advice("/*/* stuff */*/").unwrap_err();
}

#[test]
fn foreign_join_requires_multiple_identifiers() {
    parse_advice("FOREIGN_JOIN(a)").unwrap_err();
    parse_advice("FOREIGN_JOIN((a))").unwrap_err();
}

#[test]
fn occurrence_must_be_positive() {
    parse_advice("SEQ_SCAN(x#0)").unwrap_err();
}

// ── Security: deeply-nested input must not overflow the stack ──

#[test]
fn deeply_nested_sublists_rejected_not_stack_overflow() {
    // The plan-advice string is an untrusted GUC. A malicious
    // value with thousands of nested parens previously recursed
    // unboundedly (stack overflow -> backend crash). The parser
    // now caps nesting depth and returns a parse error well
    // before any stack risk. 5000 levels would overflow an
    // unguarded recursive-descent parser.
    let depth = 5000;
    let s = format!(
        "JOIN_ORDER({}{})",
        "(".repeat(depth),
        ")".repeat(depth),
    );
    // Must return an Err (not panic, not crash).
    let result = parse_advice(&s);
    assert!(
        result.is_err(),
        "deeply-nested advice should be rejected, not parsed",
    );
}

#[test]
fn legitimate_nesting_still_parses() {
    // Real advice nests only a level or two; that must still work.
    parse_advice("FOREIGN_JOIN((a b))").unwrap();
    parse_advice("JOIN_ORDER((a b) c)").unwrap();
}
