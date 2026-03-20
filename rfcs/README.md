# RA RFC Process

## When to Write an RFC

An RFC (Request for Comments) is required for:

- **Major features** estimated at >1000 lines of code
- **Breaking changes** to public APIs, rule format, or configuration
- **Architectural changes** affecting multiple crates or system boundaries
- **New crates** added to the workspace
- **New external dependencies** that affect build or runtime

An RFC is *not* required for:

- Bug fixes
- Small features or enhancements contained within a single crate
- Documentation improvements
- Performance optimizations that don't change APIs
- Adding new optimization rules within existing categories

## RFC Lifecycle

```
Draft --> Discussion --> Decision --> Implementation --> Archive
```

1. **Draft**: Author writes the RFC using `TEMPLATE.md` and opens a PR
   adding `rfcs/NNNN-short-name.md`. The status field is `Draft`.

2. **Discussion**: Reviewers comment on the PR. The author revises
   the RFC based on feedback. Significant alternatives should be
   captured in the "Rationale and alternatives" section.

3. **Decision**: A maintainer approves or rejects the RFC.
   - **Accepted**: Status changes to `Accepted`. The PR is merged.
   - **Rejected**: Status changes to `Rejected`. The file moves to
     `rfcs/_rejected/` and the PR is merged for historical record.

4. **Implementation**: Work begins. The RFC status changes to
   `Underway` while implementation is in progress, then to
   `Implemented` once complete. Tracking issues or PRs are linked
   in the RFC header.

5. **Archive**: Completed RFCs are moved to `rfcs/_accepted/YYYY-MM/`
   after a settling period following implementation.

## Numbering

RFCs are numbered sequentially starting from 0001. The number is
assigned when the draft PR is opened. Numbers are never reused,
even for rejected RFCs.

File names follow the pattern: `NNNN-short-kebab-case-name.md`

## Retroactive RFCs

Features that were implemented before the RFC process was established
may have retroactive RFCs written to document design decisions. These
are marked with `Type: Retroactive` in their header and tend to be
shorter since the implementation already exists.

## Status Values

| Status | Meaning |
|--------|---------|
| Draft | Under initial authoring |
| Under Review | Open for discussion |
| Accepted | Approved, not yet started |
| Underway | Implementation in progress |
| Implemented | Feature is complete and merged |
| Rejected | Proposal was declined |
