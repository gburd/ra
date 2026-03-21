# RA Optimizer RFC Process

This document describes the Request for Comments (RFC) process for proposing and implementing major features in the RA optimizer project.

## When to Write an RFC

Write an RFC for:
- **Major features** - New optimizer rules, execution strategies, or subsystems
- **Breaking changes** - API changes, configuration format changes, removal of features
- **Architecture decisions** - New dependencies, significant refactoring, system boundaries
- **Performance trade-offs** - Changes that significantly impact memory usage or query optimization time
- **Integration points** - New database dialect support, external system integrations

Skip the RFC for:
- Bug fixes (unless they require architectural changes)
- Documentation improvements
- Performance optimizations that don't change interfaces
- Adding tests or examples
- Minor refactoring

## RFC Lifecycle

```
Draft → Discussion → Decision → Implementation → Archive
```

### 1. Draft
- Fork the repository and create a new branch
- Copy `TEMPLATE.md` to `text/NNNN-feature-name.md` (use next sequential number)
- Fill out the template completely
- Submit a pull request with `[RFC]` prefix in the title

### 2. Discussion
- Community reviews and provides feedback via PR comments
- Author iterates on the design based on feedback
- Minimum discussion period: 7 days for standard RFCs, 14 days for breaking changes

### 3. Decision
- Maintainers make final decision: Accept, Reject, or Request Changes
- Accepted RFCs get Status changed to "Accepted" and merged to main
- Rejected RFCs are moved to `_rejected/` with explanation

### 4. Implementation
- Create tracking issue referencing the RFC
- Implement the feature following the approved design
- Update RFC status to "Implemented" when complete

### 5. Archive
- Implemented RFCs move to `_accepted/YYYY-MM/commit-NNNN-name.md`
- Include the implementation commit SHA in the filename
- Original RFC remains for historical reference

## RFC Numbering

- Sequential numbering starting from 0001
- No gaps in numbering (even for rejected RFCs)
- Format: `NNNN-descriptive-name.md`
- Leading zeros for consistent sorting

## How to Propose an RFC

1. **Check existing RFCs** - Ensure your idea isn't already proposed or rejected
2. **Discuss informally** - Open an issue for initial feedback before writing the full RFC
3. **Write the RFC** - Use the template, be thorough but concise
4. **Submit PR** - Title: `[RFC] NNNN: Feature Name`
5. **Engage in discussion** - Respond to feedback, iterate on the design
6. **Implementation** - Once accepted, implement following the approved design

## How to Review RFCs

When reviewing an RFC, consider:

- **Motivation** - Is the problem clearly stated? Is it worth solving?
- **Design** - Is the solution well-thought-out? Are edge cases considered?
- **Alternatives** - Have other approaches been fairly evaluated?
- **Complexity** - Is the complexity justified by the benefits?
- **Compatibility** - How does this affect existing users and systems?
- **Implementation** - Is the implementation plan realistic?

Provide constructive feedback focusing on:
- Technical merit over style preferences
- Specific concerns with examples
- Suggestions for improvement
- Recognition of good ideas

## Template Usage

The `TEMPLATE.md` file provides the standard structure for all RFCs. Every section should be addressed, even if briefly. Key sections:

- **Summary** - One paragraph overview for quick understanding
- **Motivation** - The "why" behind the proposal
- **Guide-level explanation** - How users will interact with the feature
- **Reference-level explanation** - Technical implementation details
- **Drawbacks** - Honest assessment of downsides
- **Rationale and alternatives** - Why this approach over others
- **Prior art** - Learning from other systems
- **Unresolved questions** - What still needs to be figured out
- **Future possibilities** - Natural extensions (but not in initial scope)

## Special RFC Categories

### Retroactive RFCs
For features already implemented before the RFC process was established. These document the rationale and design of existing features for historical context.

### Security RFCs
RFCs with security implications should be marked `[SECURITY]` and require additional review from security-focused contributors.

### Performance RFCs
RFCs focused on performance improvements should include:
- Benchmark methodology
- Expected improvements with specific metrics
- Trade-offs in terms of memory, CPU, and complexity

## RFC Status Definitions

- **Draft** - Initial proposal, under active development
- **Under Review** - Ready for community feedback
- **Accepted** - Approved for implementation
- **Rejected** - Not accepted (with documented reasons)
- **Implemented** - Feature is complete and merged
- **Withdrawn** - Author chose to withdraw the proposal

## References

This process is inspired by:
- [Rust RFC Process](https://github.com/rust-lang/rfcs)
- [React RFC Process](https://github.com/reactjs/rfcs)
- [Ember RFC Process](https://github.com/emberjs/rfcs)