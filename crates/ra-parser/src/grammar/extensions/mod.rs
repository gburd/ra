//! Third-party extension modules for PostgreSQL and other databases.
//!
//! These extensions add support for non-standard SQL features provided by
//! database extensions like PostGIS, TimescaleDB, DocumentDB, etc.

pub mod documentdb;

pub use documentdb::DocumentDBExtension;
