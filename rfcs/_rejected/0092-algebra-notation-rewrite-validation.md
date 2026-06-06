# RFC 0092: Validate Rewrites Against `## Relational Algebra` Notation

- Start Date: 2026-06-06
- Author: Ra Team
- Status: Rejected
- Tracking Issue: n/a
- Related: RFC 0090 (Rule-Driven Engine), RFC 0091 (Cost Models as Rules)

## Summary

Each `.rra` rule file carries a `## Relational Algebra` section written in
informal mathematical notation (σ, π, ⋈, ∪, …) describing the transformation,
alongside a `## Implementation` section containing the executable egg rewrite.
This RFC proposed mechanically validating the implementation against the
algebra notation — parsing the σ/π/⋈ expressions into a comparable form and
checking that the compiled rewrite agrees with the declared algebraic identity.

**This RFC is rejected.** The `## Relational Algebra` section is free-form
human documentation with no enforced grammar, and the cost of building (and
maintaining) a parser/equivalence-checker for it is not justified by the
correctness value it would add over the guards Ra already has.

## Motivation

The original idea was attractive on its face: the rule files already state the
transformation twice — once formally (`## Relational Algebra`) and once
executably (`## Implementation`) — so a checker that proves the two agree would
turn the prose into a machine-verified specification and catch rules whose
implementation drifts from their stated intent.

### Goals (as originally conceived)

1. Detect rules whose `## Implementation` rewrite does not match the algebraic
   identity in `## Relational Algebra`.
2. Make the formal-algebra section load-bearing rather than decorative.
3. Provide a third, independent correctness signal beyond tests and the
   idempotence/property suite.

## Why this is rejected

### 1. The notation is not a language

`## Relational Algebra` blocks are free-form. Across the ~1,360 rules that have
one, the notation varies in essentially every dimension:

- **Operator vocabulary**: σ/π/⋈/⨝/▷/⋉/⋊/∪/∩/− plus ad-hoc subscripts,
  arrows, and English connectives ("where", "if", "produces").
- **Predicate syntax**: sometimes SQL-ish (`a.x = b.y`), sometimes set-builder,
  sometimes prose ("a join predicate referencing only the left relation").
- **Metavariables**: `R`, `S`, `E`, `p`, `θ`, `c` with no declared binding to
  the rewrite's actual pattern variables.
- **Direction and conditions**: preconditions are frequently stated in prose
  next to the formula rather than as part of it.

Turning this into something checkable requires first *imposing* a grammar and
then *rewriting all ~1,360 sections* to conform — at which point the formal
section is no longer the human-readable doc it exists to be. The notation's
value is precisely that it is readable by people; formalizing it removes that
value to serve a checker.

### 2. Equivalence checking is the hard part, and we already have it where it counts

Even with a parsed algebra tree, "does this rewrite implement this identity?"
is relational-algebra equivalence — undecidable in general, and in the tractable
fragments still a substantial engine. Ra already exercises the property that
actually matters (the rewrite preserves results) through:

- the executable egg rewrite itself (pattern → pattern, structurally checked at
  compile time by `ra-parser/build.rs`),
- the optimizer property suite (idempotence, `extended_all_properties`, …),
- the per-rule `## Test Cases` no-panic/coverage harness
  (`crates/ra-engine/tests/rra_testcase_harness.rs`), and
- differential SQL testing (`ra-sqltest`, `ra-difftest`).

A notation-equivalence checker would re-derive a weaker version of guarantees
these already provide, while adding a large new component to maintain.

### 3. Cost/benefit is upside-down

The work (a notation grammar, a parser, a metavariable-binding pass, an
equivalence engine, plus normalizing 1,360 prose sections) is open-ended and
research-grade. The marginal correctness it buys — over tests + idempotence +
the structural compile-time check on the rewrite — is small. Engineering effort
is better spent on RFC 0091 P2 (migrating operator cost formulas into rules
with golden equivalence tests), which improves a load-bearing path.

## Rationale and alternatives

### What we do instead

- **Keep `## Relational Algebra` as human documentation.** It is valuable for
  readers and reviewers; it is displayed by `ra-cli show`. It stays inert with
  respect to the build, by design.
- **Rely on executable guards** for correctness: the compiled rewrite, the
  property/idempotence suite, the `## Test Cases` harness, and differential
  testing.

### Alternatives considered

- **Constrain the notation to a strict DSL and parse it.** Rejected: destroys
  the section's readability and forces a mass rewrite of 1,360 files for
  marginal value.
- **LLM-based "does the code match the prose?" linting.** Rejected as a
  correctness gate: non-deterministic, not reproducible in CI, and not a sound
  proof of equivalence.
- **Hand-author machine-checkable identities for a small high-value subset.**
  Not pursued now: the same assurance is obtained more cheaply by adding
  targeted differential/property tests for those specific rules.

### Impact of not doing this

None negative. The formal-algebra sections continue to serve their documentation
purpose; correctness continues to be enforced by tests, properties, and the
structural rewrite check.

## Prior art

Systems that treat algebraic rules as machine-checked artifacts (e.g. Cascades/
Calcite rule metadata, Cosette/UDP for SQL equivalence) define the rules
*directly* in a formal language rather than recovering a formal meaning from
prose. Ra's executable rewrites already are that formal artifact; the prose
algebra section is a human gloss on top, not the source of truth. The lesson
from prior art is to make the executable form authoritative — which Ra already
does — rather than to parse documentation.

## Unresolved questions

None. This decision can be revisited if the rule corpus ever adopts a strict,
enforced algebra DSL for an independent reason (e.g. rendering or teaching
tooling), at which point mechanical validation would become cheap enough to
reconsider.
