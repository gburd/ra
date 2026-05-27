#![expect(clippy::unwrap_used, reason = "test code; unwrap is the conventional shorthand for surfacing failures in tests")]
//! Parse the full corpus of advice strings from PG's regression
//! tests at `~/src/postgres/contrib/pg_plan_advice/sql/*.sql`.
//!
//! The corpus is checked into this crate at
//! `tests/data/pgpa-corpus.txt`, one advice string per line. Every
//! line that PG accepts must parse here; every line that PG
//! rejects must error here. The `KNOWN_INVALID` set below is the
//! intentional-rejection list copied from PG's `expected/syntax.out`.

use std::collections::HashSet;

use ra_plan_advice::parse_advice;

/// The advice strings PG itself rejects. Each entry is a verbatim
/// line from `tests/data/pgpa-corpus.txt`. When PG and Ra disagree,
/// either this list or the parser is wrong.
const KNOWN_INVALID: &[&str] = &[
    // Top-level grammar rejections.
    "()",
    "123",
    "/*",
    r#"JOIN_ORDER("fOO") /* oops"#,
    "/*/* stuff */*/",

    // Tag-arity / tag-shape errors.
    "JOIN_ORDER()",
    "GATHER(((x)))",
    "SEQ_SCAN((x))",
    "SEQ_SCAN(",
    "SEQ_SCAN",
    r#"SEQ_SCAN(""#,
    r#"SEQ_SCAN("")"#,
    r#"SEQ_SCAN("a""#,
    "SEQ_SCAN(#",
    "SEQUENTIAL_SCAN(x)",

    // FOREIGN_JOIN requires multiple targets in each sublist.
    "FOREIGN_JOIN(a)",
    "FOREIGN_JOIN((a))",
];

#[test]
fn every_corpus_line_matches_pg() {
    let corpus = include_str!("data/pgpa-corpus.txt");
    let invalid: HashSet<&str> = KNOWN_INVALID.iter().copied().collect();

    let mut total = 0;
    let mut accepted = 0;
    let mut rejected = 0;
    let mut surprises: Vec<String> = Vec::new();

    for line in corpus.lines() {
        // Empty lines are valid input (parses to empty advice)
        // but `KNOWN_INVALID` doesn't include them; keep counting
        // them as accepted.
        total += 1;
        let result = parse_advice(line);
        let pg_says_invalid = invalid.contains(line);

        match (result.is_ok(), pg_says_invalid) {
            (true, false) => accepted += 1,
            (false, true) => rejected += 1,
            (true, true) => surprises.push(format!(
                "expected reject, got accept: {line:?}",
            )),
            (false, false) => surprises.push(format!(
                "expected accept, got reject {:?}: {:?}",
                result.unwrap_err().message,
                line,
            )),
        }
    }

    assert!(
        surprises.is_empty(),
        "{} corpus disagreement(s):\n{}",
        surprises.len(),
        surprises.join("\n"),
    );
    assert_eq!(
        total,
        accepted + rejected,
        "every line must be classified",
    );
    assert!(total > 100, "corpus should have ~119 entries, got {total}");
}
