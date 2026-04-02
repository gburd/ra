//! Terminal UI for interactive optimizer plan monitoring.
//!
//! Provides a 4-panel ratatui interface for visualizing query
//! optimization behavior over time:
//!
//! - **Statistics panel**: table stats, staleness, confidence
//! - **Plan tree**: visual relational algebra tree
//! - **Evolution chart**: cost over optimization iterations
//! - **Execution feedback**: rule applications and diagnostics
//!
//! Supports play/pause/step controls and headless mode for
//! automated testing.

#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]

pub mod app;
pub mod event;
pub mod layout;
pub mod panels;
pub mod playback;
pub mod recorder;
pub mod setup;
// Phase 6: Timeline system (deferred)
#[cfg(feature = "timeline")]
pub mod timeline;
pub mod ui;

pub use app::{App, AppError};
pub use event::{EventHandler, TuiEvent};
pub use layout::{EditorLayout, FrameLayout, LayoutMode, PanelLayout};
pub use panels::sql_editor::{EditorMode, SqlEditor, ViMode};
pub use playback::PlaybackController;
pub use recorder::{AsciiRecorder, RecordError, record_session};
pub use setup::{SetupError, TuiConfig};
#[cfg(feature = "timeline")]
pub use timeline::{Snapshot, Timeline};
