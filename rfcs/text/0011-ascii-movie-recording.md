# RFC 0011: ASCII Movie Recording

- **Status:** Underway
- **Type:** Prospective
- **Author:** RA Contributors
- **Date:** 2026-03-20
- **Tracking:** Phase 3 of deployment plan

---

## Summary

Add the ability to record terminal sessions of RA optimizer
operations as ASCII movies (asciicast format) that can be played
back in the TUI, embedded in documentation, or shared via
asciinema.org. This provides reproducible, visual demonstrations
of query optimization without requiring a live environment.

## Motivation

Demonstrating the RA optimizer currently requires either a live
terminal session or static screenshots. ASCII movies capture the
full interactive experience -- typing queries, watching the
optimizer apply rules step by step, seeing plan diffs appear --
in a lightweight, text-based format that can be:

- Embedded in README and documentation
- Played back in the TUI for guided tutorials
- Shared as links (asciinema.org)
- Version-controlled alongside the code
- Used in CI to generate up-to-date demo recordings

## Guide-Level Explanation

### Recording

```bash
# Record an interactive session
ra-cli record --output demo.cast

# Record a scripted demonstration
ra-cli record --script demos/join-optimization.sh --output demo.cast

# Record with a specific terminal size
ra-cli record --cols 120 --rows 40 --output demo.cast
```

### Playback

```bash
# Play in terminal
ra-cli play demo.cast

# Play in the TUI
ra-cli tui --movie demo.cast

# Play at 2x speed
ra-cli play demo.cast --speed 2.0
```

### Scripted Recordings

Script files define a sequence of commands with timing:

```toml
[[steps]]
command = "ra-cli optimize 'SELECT * FROM orders JOIN customers USING (id)'"
delay_ms = 500
pause_after_ms = 2000

[[steps]]
command = "ra-cli optimize --diff colored 'SELECT * FROM orders JOIN customers USING (id) WHERE total > 100'"
delay_ms = 300
pause_after_ms = 3000
```

## Reference-Level Explanation

### Asciicast v2 Format

Recordings use the asciicast v2 format (newline-delimited JSON):

```json
{"version": 2, "width": 120, "height": 40, "timestamp": 1711000000}
[0.5, "o", "$ ra-cli optimize ..."]
[1.0, "o", "\r\nOptimizing...\r\n"]
```

### TUI Integration

The `ra-tui` crate adds:

- `MoviePlayer` widget: renders asciicast frames with timing
- `MovieRecorder`: captures terminal output during TUI sessions
- Playback controls: play, pause, seek, speed adjustment

### Script Engine

The script engine:

1. Parses the TOML script file
2. Spawns a pseudo-terminal (PTY)
3. Feeds commands with configured delays
4. Captures all output as asciicast events
5. Writes the final `.cast` file

### CI Integration

A GitHub Actions workflow generates recordings from scripts on each
release, ensuring demo movies stay current with the codebase.

## Drawbacks

- PTY handling is platform-specific (Unix only for recording;
  playback works everywhere)
- Large recordings (>5 minutes) produce files that are awkward to
  version-control
- Scripted recordings may drift from actual CLI behavior if not
  regenerated regularly

## Rationale and Alternatives

**Alternative: GIF recordings.** Higher fidelity but much larger
files, not text-searchable, and cannot be played back in a terminal.

**Alternative: SVG animations.** Render terminal output as SVG
frames. Good for web embedding but lose the terminal aesthetic and
interactivity.

The asciicast format was chosen for its ecosystem (asciinema player,
web embedding), small file size, and native terminal playback.

## Prior Art

- asciinema -- the standard tool for terminal recording
- VHS (charmbracelet) -- scripted terminal recording in Go
- terminalizer -- Node.js terminal recorder
- svg-term -- convert asciicast to SVG

## Unresolved Questions

- Should recordings include TUI mode (ratatui) or only raw CLI
  output?
- How to handle terminal color themes across different playback
  environments?
- Maximum recommended recording length before splitting into
  chapters?

## Future Possibilities

- Interactive tutorials that pause for user input
- Automated regression testing by diffing recording output
- Web UI integration for browser-based playback
- Recording library for embedding in Rust documentation tests
