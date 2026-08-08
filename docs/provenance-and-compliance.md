# Provenance, licensing, and hosting compliance

This document records Ra's stance on AI provenance, copyright/licensing, and
Codeberg's Terms of Use, so that the position is explicit and adoptable rather
than left as an open question (RA-STEERING §8; Codeberg #15).

## AI provenance

Ra is created with substantial AI assistance under the direction and review of
the Author (Greg Burd <greg@burd.me>). This is disclosed prominently in the
`README.md` "Disclosure" section (AIA-format attestation naming the model(s)
used) and is not hidden or ambiguous.

## Copyright and licensing stance (explicit)

The Author asserts copyright in the human-authored contributions and in the
selection, arrangement, and direction of the work as a whole, and licenses the
whole under a choice of Apache-2.0 / MIT / ISC (see `README.md` "Copyright and
License" and the `LICENSE-*` files). Where individual AI-generated fragments may
not be independently copyrightable under current US law, they are offered under
the same terms for the avoidance of doubt, with no additional restriction. This
is a deliberate, stated position — an adopter can rely on the triple license
regardless of how the fragment-level copyrightability question is ultimately
resolved.

## Codeberg Terms of Use compliance (Codeberg #15)

Codeberg's Terms of Use restrict AI-generated content. Ra's position:

- **Disclosure, not concealment.** The AI contribution is disclosed in the
  README with an AIA attestation. Codeberg's concern is undisclosed or
  low-effort mass-generated content; Ra is human-directed, human-reviewed, and
  openly attributed.
- **Human authorship and review.** Every change is directed, reviewed, and
  merged by the Author; the work is not autonomous bulk generation. The commit
  history, the correctness gates (`ra verify`), and the mechanical CI checks are
  the evidence of review.
- **Action if this becomes a hosting problem.** The canonical remote is
  Codeberg, mirrored to GitHub. If Codeberg determines the disclosed AI
  provenance is incompatible with its ToU, the project will move its canonical
  home to a compliant host (the GitHub mirror already exists) rather than remove
  the disclosure. Transparency is not negotiable; hosting is.

This section is the "verify compliance before it becomes a hosting problem"
follow-through requested in RA-STEERING §8: the stance is stated, the disclosure
is in place, and the contingency is defined.

## The "IN-subquery 22ms vs 11ms" figures (Codeberg #16)

RA-STEERING §8 flagged two different published endpoints (22 ms, 11 ms) for the
same IN-subquery optimization, and one framing ("785× faster") that compared Ra
to Ra rather than to PostgreSQL. The §3 truth-reset removed those marketing
claims from the repository: **no conflicting performance figure for that fix is
published anywhere in the tree today** (verified by grep over `README.md`,
`CHANGELOG.md`, and `benchmarks/`). Per §3.2 and §8, that optimization is a bug
fix (Ra removing a pathology in its own planning), not a comparison to
PostgreSQL, and it will only be re-published — if at all — as an end-to-end
(plan + execute) measurement against native PostgreSQL under a committed
harness, once correctness parity (Gate 1) is reached. No performance number is
published before then.
