//! Panel rendering modules for the 4-panel TUI layout.
//!
//! Each panel is a self-contained rendering module that takes
//! snapshot data and draws into a ratatui [`Frame`] at a given
//! [`Rect`].

pub mod evolution;
pub mod feedback;
pub mod plan_tree;
pub mod sql_editor;
pub mod statistics;
