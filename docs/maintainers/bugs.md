# Bugs & Issues

Track known bugs, issues, and their resolution status.

## Issue Tracker

All bugs and issues are tracked on Codeberg:

**[→ View Open Issues](https://codeberg.org/gregburd/ra/issues)**

---

## Known Issues

### Critical (P0)

- **ra-web blank page** - Web UI not loading ([#TBD](https://codeberg.org/gregburd/ra/issues))
  - Status: Investigating
  - Likely: Frontend build or static file serving issue

- **ra-cli test failures** - 3 tests failing in migrate_commands ([#TBD](https://codeberg.org/gregburd/ra/issues))
  - `float_threshold_narrowing`
  - `narrowed_threshold_detected`
  - `optional_to_required_is_data_loss_risk`
  - Status: Needs investigation

### High Priority (P1)

- **ra-pg-extension untested** - Integration tests not run yet
  - Status: Tests written, need to run: `cargo pgrx test pg17`

---

## Reporting Bugs

When reporting a bug, include:

1. **Description**: What happened?
2. **Expected behavior**: What should have happened?
3. **Steps to reproduce**: How to trigger the bug?
4. **Environment**: OS, Rust version, RA version
5. **Logs/Errors**: Full error messages
6. **Query**: If query-related, include SQL

**Template**: Use [bug report template](https://codeberg.org/gregburd/ra/issues/new?template=bug_report.md)

---

## Bug Priority Levels

- **P0 - Critical**: Crashes, data loss, security issues
- **P1 - High**: Major functionality broken
- **P2 - Medium**: Minor functionality broken, workarounds exist
- **P3 - Low**: Cosmetic issues, minor annoyances

---

## Security Issues

**DO NOT** report security vulnerabilities in public issues.

Email: [security@ra-optimizer.org](mailto:security@ra-optimizer.org) (if set up)

Or: Open a confidential issue on Codeberg

---

## Related Resources

- **[Chores](./chores.md)** - Small tasks and improvements
- **[RFCs](./rfcs/)** - Feature proposals
