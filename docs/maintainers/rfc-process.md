# RFC Process

This document describes how to propose, review, and implement RFCs (Request for Comments) in the Ra optimizer project.

## What is an RFC?

An RFC is a design document that proposes a new feature, optimization strategy, architectural change, or significant modification to the Ra optimizer. RFCs serve several purposes:

1. **Document design decisions** before implementation begins
2. **Facilitate discussion** among maintainers and contributors
3. **Provide historical context** for future maintenance
4. **Enable parallel work** by clearly defining interfaces and expectations

## When to Write an RFC

Write an RFC for:

- **New optimization rules** or rule categories
- **Database-specific optimizations** (PostgreSQL extensions, MongoDB features, etc.)
- **Cost model changes** that affect multiple query patterns
- **Architectural changes** to the optimizer core
- **New dialect support** or translation strategies
- **Performance features** (adaptive execution, resource budgeting, etc.)
- **API changes** that affect external integrations

You typically don't need an RFC for:

- Bug fixes
- Documentation improvements
- Test additions
- Minor refactoring within existing modules
- Performance optimizations that don't change APIs

## RFC Template

Create a new file in `rfcs/text/` with the naming pattern `NNNN-short-title.md`:

```markdown
# RFC NNNN: Title Here

- Start Date: YYYY-MM-DD
- Author: Your Name
- Status: Proposed
- Tracking Issue: TBD

## Summary

One paragraph explanation of the feature or change.

## Motivation

Why are we doing this? What use cases does it support? What problems does it solve?
Include concrete examples and expected performance improvements.

## Guide-level explanation

Explain the proposal as if teaching it to another Ra developer. This should include:

- How the feature works from a user perspective
- Example queries that benefit
- Integration points with existing systems
- Any new concepts or terminology

## Reference-level explanation

This is the technical portion of the RFC:

- Data structures and types
- Algorithms and their complexity
- Integration with existing code
- Edge cases and error handling
- Performance characteristics

## Drawbacks

Why should we *not* do this?

- Implementation complexity
- Maintenance burden
- Performance costs in other scenarios
- Breaking changes

## Rationale and alternatives

- Why is this design the best in the space of possible designs?
- What other designs have been considered and what is the rationale for not choosing them?
- What is the impact of not doing this?

## Prior art

What prior work exists? This can include:

- Academic papers
- Other database optimizers (PostgreSQL, SQL Server, Oracle, etc.)
- Related RFCs in this project
- Industry implementations

## Unresolved questions

What parts of the design do we need to resolve during the RFC process?

## Future possibilities

What natural extensions or related features could be built on this work?
```

## RFC Lifecycle

### 1. Proposed

A new RFC is created and submitted for discussion. During this phase:

- The RFC number is assigned sequentially
- Maintainers review and provide feedback
- The author revises based on discussion
- Alternative designs are explored

**How to submit:**

```bash
# Create your RFC
vim rfcs/text/0086-my-feature.md

# Submit a pull request
git checkout -b rfc-0086-my-feature
git add rfcs/text/0086-my-feature.md
git commit -m "RFC 0086: My Feature"
git push origin rfc-0086-my-feature
# Open PR with label "RFC"
```

### 2. Draft

The RFC is approved in principle but needs refinement:

- Core design is accepted
- Implementation details being worked out
- May have unresolved questions
- Can proceed with experimental implementation

**Status change:** Maintainer updates `Status: Draft` in the RFC frontmatter.

### 3. Active

The RFC is approved and implementation is in progress:

- Design is finalized
- Implementation PR is open or in progress
- Tests are being written
- Documentation is being prepared

**Status change:** When implementation PR is created, update to `Status: Active`.

### 4. Complete

The RFC is fully implemented and merged:

- Code is merged to main branch
- Tests are passing
- Documentation is updated
- Integration is verified

**Status change:** When implementation is merged, update to `Status: Complete`.

### 5. Deprecated

The RFC is no longer relevant:

- Feature was removed
- Better alternative was implemented
- Design was superseded by another RFC

**Status change:** Add deprecation note explaining why and what replaced it.

## Best Practices

### Be Specific

Bad:
> This RFC proposes to make joins faster.

Good:
> This RFC proposes a hash join spill-to-disk strategy that activates when memory
> pressure exceeds 80% of the configured budget, with cost model adjustments that
> account for random I/O to temporary storage.

### Show Benchmarks

Include concrete performance data when possible:

```markdown
## Expected Impact

Benchmarked on TPC-H Scale Factor 10 with 4GB memory limit:

| Query | Before | After | Improvement |
|-------|--------|-------|-------------|
| Q1    | 45s    | 42s   | 7%          |
| Q13   | OOM    | 125s  | Completes   |
| Q21   | 180s   | 95s   | 47%         |
```

### Reference Existing Code

Link to specific files and line numbers:

```markdown
The implementation will extend `JoinStrategy` in `crates/ra-engine/src/join.rs:145`
and integrate with the memory budget tracking in `budget.rs:67`.
```

### Consider Edge Cases

Think through failure modes:

- What happens when statistics are missing?
- How does this interact with distributed execution?
- What are the memory/CPU tradeoffs?
- How does this affect plan cache invalidation?

### Document Dependencies

Be explicit about what this RFC builds on or blocks:

```markdown
## Dependencies

- **Requires:** RFC 0074 (Resource-Aware Scheduling) for memory pressure detection
- **Blocks:** RFC 0089 (Adaptive Join Reordering) which needs spill cost estimates
- **Related:** RFC 0070 (Memory-Aware Joins) uses similar heuristics
```

## RFC Review Process

1. **Initial Review** (1-3 days): Maintainers check for completeness and clarity
2. **Community Discussion** (1-2 weeks): Open for comments and questions
3. **Revision Period** (as needed): Author addresses feedback
4. **Approval Vote** (3 days): Maintainers vote to approve or request changes
5. **Implementation** (varies): RFC moves to Active status

## Updating Existing RFCs

RFCs can be amended after approval:

- Add implementation notes as sections
- Document deviations from original design
- Link to related RFCs created later
- Add performance results

Major changes require a new RFC that supersedes the old one.

## Questions?

Reach out to maintainers via:

- GitHub discussions
- Issue tracker
- Project chat (if available)
