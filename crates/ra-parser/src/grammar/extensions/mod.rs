//! Third-party extension modules for PostgreSQL and other databases.
//!
//! These extensions add support for non-standard SQL features provided by
//! database extensions like PostGIS, TimescaleDB, DocumentDB, pgvector, pg_trgm, etc.

pub mod documentdb;
pub mod pg_trgm;
pub mod pgvector;

pub use documentdb::DocumentDBExtension;
pub use pg_trgm::PgTrgmExtension;
pub use pgvector::PgVectorExtension;
