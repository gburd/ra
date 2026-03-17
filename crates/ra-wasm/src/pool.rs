//! Connection pooling for WASM database adapters.
//!
//! Provides a simple pool that reuses open connections to avoid
//! repeated initialization overhead. Browser WASM contexts are
//! single-threaded, so the pool uses `RefCell` instead of a mutex.

use std::cell::RefCell;
use std::collections::VecDeque;

use crate::adapter::{ConnectionConfig, DatabaseAdapter, DatabaseEngine, QueryResult, Value};
use crate::duckdb::DuckDbAdapter;
use crate::errors::{Result, WasmDbError};
use crate::sqlite::SqliteAdapter;

/// Configuration for a connection pool.
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Base configuration for new connections.
    pub connection: ConnectionConfig,
    /// Maximum number of connections in the pool.
    pub max_connections: usize,
}

impl PoolConfig {
    /// Create a pool config with defaults (max 4 connections).
    #[must_use]
    pub fn new(connection: ConnectionConfig) -> Self {
        Self {
            connection,
            max_connections: 4,
        }
    }

    /// Set the maximum number of pooled connections.
    #[must_use]
    pub fn with_max_connections(mut self, max: usize) -> Self {
        self.max_connections = max;
        self
    }
}

/// A pool of database connections.
///
/// Connections are borrowed from the pool and returned
/// automatically when the guard is dropped.
#[derive(Debug)]
pub struct ConnectionPool {
    config: PoolConfig,
    idle: RefCell<VecDeque<Box<dyn DatabaseAdapter>>>,
    active_count: RefCell<usize>,
}

impl ConnectionPool {
    /// Create a new connection pool.
    #[must_use]
    pub fn new(config: PoolConfig) -> Self {
        Self {
            config,
            idle: RefCell::new(VecDeque::new()),
            active_count: RefCell::new(0),
        }
    }

    /// Acquire a connection from the pool.
    ///
    /// Returns an idle connection if available, otherwise creates
    /// a new one (up to `max_connections`).
    ///
    /// # Errors
    ///
    /// Returns an error if the pool is exhausted or connection
    /// creation fails.
    pub fn acquire(&self) -> Result<PooledConnection<'_>> {
        if let Some(conn) = self.idle.borrow_mut().pop_front() {
            if conn.is_open() {
                *self.active_count.borrow_mut() += 1;
                return Ok(PooledConnection {
                    pool: self,
                    conn: Some(conn),
                });
            }
        }

        let total = *self.active_count.borrow() + self.idle.borrow().len();
        if total >= self.config.max_connections {
            return Err(WasmDbError::Connection(format!(
                "pool exhausted: {total}/{} connections in use",
                self.config.max_connections
            )));
        }

        let conn = open_adapter(&self.config.connection)?;
        *self.active_count.borrow_mut() += 1;
        Ok(PooledConnection {
            pool: self,
            conn: Some(conn),
        })
    }

    fn release(&self, conn: Box<dyn DatabaseAdapter>) {
        *self.active_count.borrow_mut() -= 1;
        if conn.is_open() {
            self.idle.borrow_mut().push_back(conn);
        }
    }

    /// Number of idle connections ready for reuse.
    #[must_use]
    pub fn idle_count(&self) -> usize {
        self.idle.borrow().len()
    }

    /// Number of connections currently checked out.
    #[must_use]
    pub fn active_count(&self) -> usize {
        *self.active_count.borrow()
    }

    /// Close all idle connections in the pool.
    pub fn drain(&self) {
        let mut idle = self.idle.borrow_mut();
        for conn in idle.drain(..) {
            let _ = conn.close();
        }
    }
}

/// A borrowed connection that returns to the pool on drop.
#[derive(Debug)]
pub struct PooledConnection<'pool> {
    pool: &'pool ConnectionPool,
    conn: Option<Box<dyn DatabaseAdapter>>,
}

impl PooledConnection<'_> {
    /// Execute a SQL statement through the pooled connection.
    ///
    /// # Errors
    ///
    /// Returns an error on failure.
    pub fn execute(&self, sql: &str) -> Result<QueryResult> {
        self.adapter().execute(sql)
    }

    /// Execute a SQL query through the pooled connection.
    ///
    /// # Errors
    ///
    /// Returns an error on failure.
    pub fn query(&self, sql: &str) -> Result<QueryResult> {
        self.adapter().query(sql)
    }

    /// Execute a parameterized SQL statement.
    ///
    /// # Errors
    ///
    /// Returns an error on failure.
    pub fn execute_with_params(&self, sql: &str, params: &[Value]) -> Result<QueryResult> {
        self.adapter().execute_with_params(sql, params)
    }

    /// Execute a parameterized SQL query.
    ///
    /// # Errors
    ///
    /// Returns an error on failure.
    pub fn query_with_params(&self, sql: &str, params: &[Value]) -> Result<QueryResult> {
        self.adapter().query_with_params(sql, params)
    }

    /// The engine backing this connection.
    #[must_use]
    pub fn engine(&self) -> DatabaseEngine {
        self.adapter().engine()
    }

    fn adapter(&self) -> &dyn DatabaseAdapter {
        // Safety: conn is always Some until drop() takes it.
        self.conn.as_deref().unwrap_or_else(|| {
            unreachable!(
                "PooledConnection always has a connection \
                 until dropped"
            )
        })
    }
}

impl Drop for PooledConnection<'_> {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            self.pool.release(conn);
        }
    }
}

fn open_adapter(config: &ConnectionConfig) -> Result<Box<dyn DatabaseAdapter>> {
    match config.engine {
        DatabaseEngine::Sqlite => {
            let adapter = SqliteAdapter::open(config)?;
            Ok(Box::new(adapter))
        }
        DatabaseEngine::DuckDb => {
            let adapter = DuckDbAdapter::open(config)?;
            Ok(Box::new(adapter))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_config_defaults() {
        let config = PoolConfig::new(ConnectionConfig::sqlite_memory());
        assert_eq!(config.max_connections, 4);
        assert_eq!(config.connection.engine, DatabaseEngine::Sqlite);
    }

    #[test]
    fn pool_config_custom_max() {
        let config = PoolConfig::new(ConnectionConfig::duckdb_memory()).with_max_connections(16);
        assert_eq!(config.max_connections, 16);
    }

    #[test]
    fn pool_initial_state() {
        let pool = ConnectionPool::new(PoolConfig::new(ConnectionConfig::sqlite_memory()));
        assert_eq!(pool.idle_count(), 0);
        assert_eq!(pool.active_count(), 0);
    }

    #[test]
    fn pool_drain_empty_is_noop() {
        let pool = ConnectionPool::new(PoolConfig::new(ConnectionConfig::sqlite_memory()));
        pool.drain();
        assert_eq!(pool.idle_count(), 0);
    }
}
