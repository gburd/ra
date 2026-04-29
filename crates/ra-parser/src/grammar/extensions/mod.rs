//! Third-party extension modules for `PostgreSQL` and other databases.
//!
//! These extensions add support for non-standard SQL features provided by
//! database extensions like `PostGIS`, `TimescaleDB`, `DocumentDB`, `pgvector`, `pg_trgm`, `sqlite-vec`, etc.

pub mod documentdb;
pub mod mysql_fts;
pub mod pg_trgm;
pub mod pgvector;
pub mod sqlite_vec;
pub mod sqlserver_fts;

pub use documentdb::DocumentDBExtension;
pub use mysql_fts::MySQLFTSExtension;
pub use pg_trgm::PgTrgmExtension;
pub use pgvector::PgVectorExtension;
pub use sqlite_vec::SqliteVecExtension;
pub use sqlserver_fts::SQLServerFTSExtension;
