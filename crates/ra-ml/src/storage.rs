//! Database backend for ML models and belief networks.
//!
//! Replaces JSON file persistence with proper database storage,
//! enabling differential dataflow-driven model persistence and
//! multi-instance model sharing.

use std::str::FromStr;

use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgPoolOptions, Pool, Postgres};
use thiserror::Error;
use tracing::{debug, info};

use crate::belief_network::{BeliefNetworkState, ConditionalProbabilityTable, ExecutionObservation};
use crate::nn::FeedForwardNet;

/// Errors from storage operations.
#[derive(Debug, Error)]
pub enum StorageError {
    /// Database connection failed.
    #[error("database connection failed: {0}")]
    ConnectionFailed(String),

    /// Query execution failed.
    #[error("query failed: {0}")]
    QueryFailed(String),

    /// Serialization error.
    #[error("serialization failed: {0}")]
    SerializationFailed(String),

    /// Deserialization error.
    #[error("deserialization failed: {0}")]
    DeserializationFailed(String),

    /// Model not found.
    #[error("model not found: {0}")]
    ModelNotFound(String),

    /// Invalid configuration.
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
}

/// Database backend type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DatabaseBackend {
    /// PostgreSQL backend.
    Postgres,
}

impl FromStr for DatabaseBackend {
    type Err = StorageError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "postgres" | "postgresql" => Ok(Self::Postgres),
            _ => Err(StorageError::InvalidConfig(format!(
                "unknown backend: {s}"
            ))),
        }
    }
}

/// Storage configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Database backend type.
    pub backend: DatabaseBackend,
    /// Connection string.
    pub connection_string: String,
    /// Maximum connections in pool.
    pub max_connections: u32,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            backend: DatabaseBackend::Postgres,
            connection_string: "postgresql://localhost/ra_ml".to_string(),
            max_connections: 10,
        }
    }
}

/// Model storage backend.
pub enum ModelStorage {
    /// PostgreSQL storage.
    Postgres(Pool<Postgres>),
}

impl ModelStorage {
    /// Create a new storage backend from configuration.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if connection fails.
    pub async fn new(config: StorageConfig) -> Result<Self, StorageError> {
        match config.backend {
            DatabaseBackend::Postgres => {
                let pool = PgPoolOptions::new()
                    .max_connections(config.max_connections)
                    .connect(&config.connection_string)
                    .await
                    .map_err(|e| StorageError::ConnectionFailed(e.to_string()))?;

                Self::init_postgres(&pool).await?;

                info!("Connected to PostgreSQL storage");
                Ok(Self::Postgres(pool))
            }
        }
    }

    async fn init_postgres(pool: &Pool<Postgres>) -> Result<(), StorageError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS ml_models (
                id SERIAL PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                scope TEXT NOT NULL,
                account_id TEXT,
                project_id TEXT,
                model_data BYTEA NOT NULL,
                schema_data BYTEA NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );

            CREATE TABLE IF NOT EXISTS belief_networks (
                id SERIAL PRIMARY KEY,
                scope TEXT NOT NULL,
                account_id TEXT,
                project_id TEXT,
                network_data BYTEA NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                UNIQUE(scope, account_id, project_id)
            );

            CREATE TABLE IF NOT EXISTS execution_observations (
                id SERIAL PRIMARY KEY,
                rule_id TEXT NOT NULL,
                estimated_time_before DOUBLE PRECISION NOT NULL,
                estimated_time_after DOUBLE PRECISION NOT NULL,
                actual_time DOUBLE PRECISION,
                improved BOOLEAN NOT NULL,
                context BYTEA NOT NULL,
                timestamp BIGINT NOT NULL,
                scope TEXT NOT NULL,
                account_id TEXT,
                project_id TEXT,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );

            CREATE INDEX IF NOT EXISTS idx_observations_rule ON execution_observations(rule_id);
            CREATE INDEX IF NOT EXISTS idx_observations_scope ON execution_observations(scope, account_id, project_id);
            "#,
        )
        .execute(pool)
        .await
        .map_err(|e| StorageError::QueryFailed(e.to_string()))?;

        Ok(())
    }

    async fn init_sqlite(pool: &Pool<Sqlite>) -> Result<(), StorageError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS ml_models (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                scope TEXT NOT NULL,
                account_id TEXT,
                project_id TEXT,
                model_data BLOB NOT NULL,
                schema_data BLOB NOT NULL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS belief_networks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                scope TEXT NOT NULL,
                account_id TEXT,
                project_id TEXT,
                network_data BLOB NOT NULL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(scope, account_id, project_id)
            );

            CREATE TABLE IF NOT EXISTS execution_observations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                rule_id TEXT NOT NULL,
                estimated_time_before REAL NOT NULL,
                estimated_time_after REAL NOT NULL,
                actual_time REAL,
                improved INTEGER NOT NULL,
                context BLOB NOT NULL,
                timestamp INTEGER NOT NULL,
                scope TEXT NOT NULL,
                account_id TEXT,
                project_id TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE INDEX IF NOT EXISTS idx_observations_rule ON execution_observations(rule_id);
            CREATE INDEX IF NOT EXISTS idx_observations_scope ON execution_observations(scope, account_id, project_id);
            "#,
        )
        .execute(pool)
        .await
        .map_err(|e| StorageError::QueryFailed(e.to_string()))?;

        Ok(())
    }

    /// Save a neural network model.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if serialization or query fails.
    pub async fn save_model(
        &self,
        name: &str,
        model: &FeedForwardNet,
        schema_json: &[u8],
        scope: &str,
        account_id: Option<&str>,
        project_id: Option<&str>,
    ) -> Result<(), StorageError> {
        let model_data =
            serde_json::to_vec(model).map_err(|e| StorageError::SerializationFailed(e.to_string()))?;

        match self {
            Self::Postgres(pool) => {
                sqlx::query(
                    r#"
                    INSERT INTO ml_models (name, scope, account_id, project_id, model_data, schema_data, updated_at)
                    VALUES ($1, $2, $3, $4, $5, $6, NOW())
                    ON CONFLICT (name) DO UPDATE SET
                        model_data = EXCLUDED.model_data,
                        schema_data = EXCLUDED.schema_data,
                        updated_at = NOW()
                    "#,
                )
                .bind(name)
                .bind(scope)
                .bind(account_id)
                .bind(project_id)
                .bind(&model_data)
                .bind(schema_json)
                .execute(pool)
                .await
                .map_err(|e| StorageError::QueryFailed(e.to_string()))?;
            }
            Self::Sqlite(pool) => {
                sqlx::query(
                    r#"
                    INSERT INTO ml_models (name, scope, account_id, project_id, model_data, schema_data)
                    VALUES (?, ?, ?, ?, ?, ?)
                    ON CONFLICT (name) DO UPDATE SET
                        model_data = excluded.model_data,
                        schema_data = excluded.schema_data,
                        updated_at = CURRENT_TIMESTAMP
                    "#,
                )
                .bind(name)
                .bind(scope)
                .bind(account_id)
                .bind(project_id)
                .bind(&model_data)
                .bind(schema_json)
                .execute(pool)
                .await
                .map_err(|e| StorageError::QueryFailed(e.to_string()))?;
            }
        }

        info!(model = %name, "Saved model to database");
        Ok(())
    }

    /// Load a neural network model.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if model not found or deserialization fails.
    pub async fn load_model(&self, name: &str) -> Result<(FeedForwardNet, Vec<u8>), StorageError> {
        let (model_data, schema_data): (Vec<u8>, Vec<u8>) = match self {
            Self::Postgres(pool) => {
                sqlx::query_as(
                    "SELECT model_data, schema_data FROM ml_models WHERE name = $1",
                )
                .bind(name)
                .fetch_optional(pool)
                .await
                .map_err(|e| StorageError::QueryFailed(e.to_string()))?
                .ok_or_else(|| StorageError::ModelNotFound(name.to_string()))?
            }
            Self::Sqlite(pool) => {
                sqlx::query_as(
                    "SELECT model_data, schema_data FROM ml_models WHERE name = ?",
                )
                .bind(name)
                .fetch_optional(pool)
                .await
                .map_err(|e| StorageError::QueryFailed(e.to_string()))?
                .ok_or_else(|| StorageError::ModelNotFound(name.to_string()))?
            }
        };

        let model: FeedForwardNet = serde_json::from_slice(&model_data)
            .map_err(|e| StorageError::DeserializationFailed(e.to_string()))?;

        debug!(model = %name, "Loaded model from database");
        Ok((model, schema_data))
    }

    /// Save a belief network state.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if serialization or query fails.
    pub async fn save_belief_network(
        &self,
        state: &BeliefNetworkState,
        scope: &str,
        account_id: Option<&str>,
        project_id: Option<&str>,
    ) -> Result<(), StorageError> {
        let network_data = serde_json::to_vec(state)
            .map_err(|e| StorageError::SerializationFailed(e.to_string()))?;

        match self {
            Self::Postgres(pool) => {
                sqlx::query(
                    r#"
                    INSERT INTO belief_networks (scope, account_id, project_id, network_data, updated_at)
                    VALUES ($1, $2, $3, $4, NOW())
                    ON CONFLICT (scope, account_id, project_id) DO UPDATE SET
                        network_data = EXCLUDED.network_data,
                        updated_at = NOW()
                    "#,
                )
                .bind(scope)
                .bind(account_id)
                .bind(project_id)
                .bind(&network_data)
                .execute(pool)
                .await
                .map_err(|e| StorageError::QueryFailed(e.to_string()))?;
            }
            Self::Sqlite(pool) => {
                sqlx::query(
                    r#"
                    INSERT INTO belief_networks (scope, account_id, project_id, network_data)
                    VALUES (?, ?, ?, ?)
                    ON CONFLICT (scope, account_id, project_id) DO UPDATE SET
                        network_data = excluded.network_data,
                        updated_at = CURRENT_TIMESTAMP
                    "#,
                )
                .bind(scope)
                .bind(account_id)
                .bind(project_id)
                .bind(&network_data)
                .execute(pool)
                .await
                .map_err(|e| StorageError::QueryFailed(e.to_string()))?;
            }
        }

        info!(scope = %scope, "Saved belief network to database");
        Ok(())
    }

    /// Load a belief network state.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if not found or deserialization fails.
    pub async fn load_belief_network(
        &self,
        scope: &str,
        account_id: Option<&str>,
        project_id: Option<&str>,
    ) -> Result<BeliefNetworkState, StorageError> {
        let (network_data,): (Vec<u8>,) = match self {
            Self::Postgres(pool) => {
                sqlx::query_as(
                    r#"
                    SELECT network_data FROM belief_networks
                    WHERE scope = $1 AND account_id IS NOT DISTINCT FROM $2 AND project_id IS NOT DISTINCT FROM $3
                    "#,
                )
                .bind(scope)
                .bind(account_id)
                .bind(project_id)
                .fetch_optional(pool)
                .await
                .map_err(|e| StorageError::QueryFailed(e.to_string()))?
                .ok_or_else(|| StorageError::ModelNotFound(format!("belief network for scope {scope}")))?
            }
            Self::Sqlite(pool) => {
                sqlx::query_as(
                    "SELECT network_data FROM belief_networks WHERE scope = ? AND account_id IS ? AND project_id IS ?",
                )
                .bind(scope)
                .bind(account_id)
                .bind(project_id)
                .fetch_optional(pool)
                .await
                .map_err(|e| StorageError::QueryFailed(e.to_string()))?
                .ok_or_else(|| StorageError::ModelNotFound(format!("belief network for scope {scope}")))?
            }
        };

        let state: BeliefNetworkState = serde_json::from_slice(&network_data)
            .map_err(|e| StorageError::DeserializationFailed(e.to_string()))?;

        debug!(scope = %scope, "Loaded belief network from database");
        Ok(state)
    }

    /// Store execution observations.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if query fails.
    pub async fn store_observations(
        &self,
        observations: &[ExecutionObservation],
        scope: &str,
        account_id: Option<&str>,
        project_id: Option<&str>,
    ) -> Result<(), StorageError> {
        for obs in observations {
            let context_data = serde_json::to_vec(&obs.context)
                .map_err(|e| StorageError::SerializationFailed(e.to_string()))?;

            match self {
                Self::Postgres(pool) => {
                    sqlx::query(
                        r#"
                        INSERT INTO execution_observations
                        (rule_id, estimated_time_before, estimated_time_after, actual_time, improved, context, timestamp, scope, account_id, project_id)
                        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                        "#,
                    )
                    .bind(&obs.rule_id)
                    .bind(obs.estimated_time_before)
                    .bind(obs.estimated_time_after)
                    .bind(obs.actual_time)
                    .bind(obs.improved)
                    .bind(&context_data)
                    .bind(obs.timestamp)
                    .bind(scope)
                    .bind(account_id)
                    .bind(project_id)
                    .execute(pool)
                    .await
                    .map_err(|e| StorageError::QueryFailed(e.to_string()))?;
                }
                Self::Sqlite(pool) => {
                    sqlx::query(
                        r#"
                        INSERT INTO execution_observations
                        (rule_id, estimated_time_before, estimated_time_after, actual_time, improved, context, timestamp, scope, account_id, project_id)
                        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                        "#,
                    )
                    .bind(&obs.rule_id)
                    .bind(obs.estimated_time_before)
                    .bind(obs.estimated_time_after)
                    .bind(obs.actual_time)
                    .bind(obs.improved)
                    .bind(&context_data)
                    .bind(obs.timestamp)
                    .bind(scope)
                    .bind(account_id)
                    .bind(project_id)
                    .execute(pool)
                    .await
                    .map_err(|e| StorageError::QueryFailed(e.to_string()))?;
                }
            }
        }

        debug!(count = %observations.len(), "Stored execution observations");
        Ok(())
    }

    /// Load recent execution observations.
    ///
    /// # Errors
    ///
    /// Returns `StorageError` if query fails.
    pub async fn load_observations(
        &self,
        rule_id: Option<&str>,
        scope: &str,
        account_id: Option<&str>,
        project_id: Option<&str>,
        limit: i64,
    ) -> Result<Vec<ExecutionObservation>, StorageError> {
        let rows: Vec<(String, f64, f64, Option<f64>, bool, Vec<u8>, i64)> = match (self, rule_id) {
            (Self::Postgres(pool), Some(rule)) => {
                sqlx::query_as(
                    r#"
                    SELECT rule_id, estimated_time_before, estimated_time_after, actual_time, improved, context, timestamp
                    FROM execution_observations
                    WHERE rule_id = $1 AND scope = $2 AND account_id IS NOT DISTINCT FROM $3 AND project_id IS NOT DISTINCT FROM $4
                    ORDER BY created_at DESC
                    LIMIT $5
                    "#,
                )
                .bind(rule)
                .bind(scope)
                .bind(account_id)
                .bind(project_id)
                .bind(limit)
                .fetch_all(pool)
                .await
                .map_err(|e| StorageError::QueryFailed(e.to_string()))?
            }
            (Self::Postgres(pool), None) => {
                sqlx::query_as(
                    r#"
                    SELECT rule_id, estimated_time_before, estimated_time_after, actual_time, improved, context, timestamp
                    FROM execution_observations
                    WHERE scope = $1 AND account_id IS NOT DISTINCT FROM $2 AND project_id IS NOT DISTINCT FROM $3
                    ORDER BY created_at DESC
                    LIMIT $4
                    "#,
                )
                .bind(scope)
                .bind(account_id)
                .bind(project_id)
                .bind(limit)
                .fetch_all(pool)
                .await
                .map_err(|e| StorageError::QueryFailed(e.to_string()))?
            }
            (Self::Sqlite(pool), Some(rule)) => {
                sqlx::query_as(
                    r#"
                    SELECT rule_id, estimated_time_before, estimated_time_after, actual_time, improved, context, timestamp
                    FROM execution_observations
                    WHERE rule_id = ? AND scope = ? AND account_id IS ? AND project_id IS ?
                    ORDER BY created_at DESC
                    LIMIT ?
                    "#,
                )
                .bind(rule)
                .bind(scope)
                .bind(account_id)
                .bind(project_id)
                .bind(limit)
                .fetch_all(pool)
                .await
                .map_err(|e| StorageError::QueryFailed(e.to_string()))?
            }
            (Self::Sqlite(pool), None) => {
                sqlx::query_as(
                    r#"
                    SELECT rule_id, estimated_time_before, estimated_time_after, actual_time, improved, context, timestamp
                    FROM execution_observations
                    WHERE scope = ? AND account_id IS ? AND project_id IS ?
                    ORDER BY created_at DESC
                    LIMIT ?
                    "#,
                )
                .bind(scope)
                .bind(account_id)
                .bind(project_id)
                .bind(limit)
                .fetch_all(pool)
                .await
                .map_err(|e| StorageError::QueryFailed(e.to_string()))?
            }
        };

        let observations: Vec<ExecutionObservation> = rows
            .into_iter()
            .filter_map(|(rule_id, time_before, time_after, actual_time, improved, context_data, timestamp)| {
                let context: Vec<f64> = serde_json::from_slice(&context_data).ok()?;
                Some(ExecutionObservation {
                    rule_id,
                    estimated_time_before: time_before,
                    estimated_time_after: time_after,
                    actual_time,
                    improved,
                    context,
                    timestamp,
                })
            })
            .collect();

        debug!(count = %observations.len(), "Loaded execution observations");
        Ok(observations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn storage_config_default() {
        let config = StorageConfig::default();
        assert_eq!(config.backend, DatabaseBackend::Sqlite);
        assert_eq!(config.max_connections, 10);
    }

    #[test]
    fn database_backend_from_str() {
        assert_eq!(
            DatabaseBackend::from_str("postgres").unwrap(),
            DatabaseBackend::Postgres
        );
        assert_eq!(
            DatabaseBackend::from_str("sqlite").unwrap(),
            DatabaseBackend::Sqlite
        );
        assert!(DatabaseBackend::from_str("invalid").is_err());
    }
}
