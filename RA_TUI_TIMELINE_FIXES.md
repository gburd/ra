# ra-tui Timeline Feature Gate Fixes

## Summary

Fixed all timeline-related compilation errors in `/home/gburd/ws/ra/crates/ra-tui/src/` by adding proper `#[cfg(feature = "timeline")]` gates to imports, structs, implementations, and functions that depend on timeline types.

## Changes Made

### 1. `/home/gburd/ws/ra/crates/ra-tui/src/app.rs`
- Added `#[cfg(feature = "timeline")]` to `use crate::timeline::Timeline` import (line 29)
- Added `#[cfg(feature = "timeline")]` to `pub struct App` definition (line 112)
- Added `#[cfg(feature = "timeline")]` to `impl App` block (line 137)

### 2. `/home/gburd/ws/ra/crates/ra-tui/src/panels/feedback.rs`
- Added `#[cfg(feature = "timeline")]` to `use crate::timeline::{InvalidationTrigger, Snapshot}` import (line 12)
- Fixed type inference on line 59: Changed `rule.clone()` to `rule.clone().to_string()`
- Added `#[cfg(feature = "timeline")]` to `pub fn render()` (line 14)
- Added `#[cfg(feature = "timeline")]` to `pub fn build_feedback_lines()` (line 39)
- Added `#[cfg(feature = "timeline")]` to `pub fn invalidation_icon_and_color()` (line 144)
- Changed test module from `#[cfg(test)]` to `#[cfg(all(test, feature = "timeline"))]` (line 168)

### 3. `/home/gburd/ws/ra/crates/ra-tui/src/panels/statistics.rs`
- Added `#[cfg(feature = "timeline")]` to `use crate::timeline::{Change, ChangeSeverity, ChangeKind, TableStatEntry}` import (line 12)
- Added `#[cfg(feature = "timeline")]` to `pub fn render()` (line 14)
- Added `#[cfg(feature = "timeline")]` to `fn render_changes()` (line 145)
- Added `#[cfg(feature = "timeline")]` to `pub fn change_icon_and_color()` (line 193)
- Added `#[cfg(feature = "timeline")]` to `pub fn severity_indicator()` (line 209)
- Added `#[cfg(feature = "timeline")]` to `pub fn severity_color()` (line 219)
- Changed test module from `#[cfg(test)]` to `#[cfg(all(test, feature = "timeline"))]` (line 228)

### 4. `/home/gburd/ws/ra/crates/ra-tui/src/setup.rs`
- Added `#[cfg(feature = "timeline")]` to `use crate::timeline::Timeline` import (line 8)
- Added `#[cfg(feature = "timeline")]` to `pub struct TuiConfig` definition (line 11)
- Added `#[cfg(feature = "timeline")]` to `impl TuiConfig` block (line 23)
- Added `#[cfg(feature = "timeline")]` to `pub fn load_timeline_json()` (line 70)
- Changed test module from `#[cfg(test)]` to `#[cfg(all(test, feature = "timeline"))]` (line 106)

### 5. `/home/gburd/ws/ra/crates/ra-tui/src/lib.rs`
- Added `#[cfg(feature = "timeline")]` to `pub use app::{App, AppError}` (line 34)
- Split `pub use setup::{SetupError, TuiConfig}` into:
  - `pub use setup::SetupError` (unconditional, line 38)
  - `#[cfg(feature = "timeline")] pub use setup::TuiConfig` (conditional, lines 39-40)
- Timeline and Snapshot exports were already properly gated (lines 41-42)

## Expected Behavior

After these changes:

1. **Without timeline feature**: `cargo build -p ra-tui` should succeed
   - Timeline module is not compiled
   - App, TuiConfig, and timeline-dependent functions are not available
   - SetupError remains available for general error handling

2. **With timeline feature**: `cargo build -p ra-tui --features timeline` should succeed
   - Full timeline functionality available
   - All tests pass

## Testing Commands

```bash
# Build without timeline feature
cargo build -p ra-tui

# Build with timeline feature
cargo build -p ra-tui --features timeline

# Test without timeline feature
cargo test -p ra-tui

# Test with timeline feature
cargo test -p ra-tui --features timeline
```

## Files Modified

- `/home/gburd/ws/ra/crates/ra-tui/src/app.rs`
- `/home/gburd/ws/ra/crates/ra-tui/src/panels/feedback.rs`
- `/home/gburd/ws/ra/crates/ra-tui/src/panels/statistics.rs`
- `/home/gburd/ws/ra/crates/ra-tui/src/setup.rs`
- `/home/gburd/ws/ra/crates/ra-tui/src/lib.rs`

## Type Inference Fix

Fixed the type inference error in `/home/gburd/ws/ra/crates/ra-tui/src/panels/feedback.rs` line 59 by explicitly converting to String:
```rust
// Before:
Span::styled(rule.clone(), Style::default().fg(Color::Cyan))

// After:
Span::styled(rule.clone().to_string(), Style::default().fg(Color::Cyan))
```

This ensures the type is unambiguous for the Span constructor.
