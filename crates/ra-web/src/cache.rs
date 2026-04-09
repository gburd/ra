//! Redis-based result caching for query plans.

use redis::{aio::ConnectionManager, AsyncCommands};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const CACHE_TTL: u64 = 3600; // 1 hour
const CACHE_KEY_PREFIX: &str = "explain:";

/// A cached EXPLAIN result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedPlan {
    pub plan: serde_json::Value,
    pub engine: String,
}

/// Generate a SHA256 hash of the SQL query to use as cache key.
fn generate_cache_key(sql: &str, engine: &str, analyze: bool) -> String {
    let mut hasher = Sha256::new();
    hasher.update(sql.as_bytes());
    hasher.update(engine.as_bytes());
    hasher.update(if analyze { &b"analyze"[..] } else { &b"noanalyze"[..] });
    let hash = hasher.finalize();
    format!("{}{:x}", CACHE_KEY_PREFIX, hash)
}

/// Retrieve a cached plan from Redis.
pub async fn get_cached_plan(
    redis: &mut ConnectionManager,
    sql: &str,
    engine: &str,
    analyze: bool,
) -> Result<Option<CachedPlan>, redis::RedisError> {
    let key = generate_cache_key(sql, engine, analyze);

    let json: Option<String> = redis.get(&key).await?;

    if let Some(json) = json {
        match serde_json::from_str::<CachedPlan>(&json) {
            Ok(cached) => {
                tracing::info!("Cache hit for key={}", key);
                Ok(Some(cached))
            }
            Err(e) => {
                tracing::warn!("Failed to deserialize cached plan: {}", e);
                Ok(None)
            }
        }
    } else {
        tracing::debug!("Cache miss for key={}", key);
        Ok(None)
    }
}

/// Store a plan result in Redis cache.
pub async fn cache_plan(
    redis: &mut ConnectionManager,
    sql: &str,
    engine: &str,
    analyze: bool,
    plan: &serde_json::Value,
) -> Result<(), redis::RedisError> {
    let key = generate_cache_key(sql, engine, analyze);

    let cached = CachedPlan {
        plan: plan.clone(),
        engine: engine.to_string(),
    };

    let json = match serde_json::to_string(&cached) {
        Ok(json) => json,
        Err(e) => {
            tracing::error!("Failed to serialize plan for caching: {}", e);
            return Ok(());
        }
    };

    redis.set_ex::<_, _, ()>(&key, json, CACHE_TTL).await?;

    tracing::info!("Cached plan with key={}, ttl={}s", key, CACHE_TTL);

    Ok(())
}
