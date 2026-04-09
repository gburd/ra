//! Tests for the Redis caching layer.
//!
//! This module tests:
//! - Cache key generation
//! - Cache hit/miss behavior
//! - Cache serialization/deserialization
//! - CachedPlan structure
//!
//! Note: Integration tests requiring actual Redis connections are gated behind
//! the `integration-tests` feature flag.

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::cache::CachedPlan;

    #[test]
    fn test_cached_plan_serialization() {
        let plan = CachedPlan {
            plan: json!({"node": "SeqScan", "cost": 10.5}),
            engine: "postgresql".to_string(),
        };

        let json_str = serde_json::to_string(&plan).expect("Failed to serialize");
        assert!(json_str.contains("SeqScan"));
        assert!(json_str.contains("postgresql"));
    }

    #[test]
    fn test_cached_plan_deserialization() {
        let json_str = r#"{"plan":{"node":"SeqScan"},"engine":"postgresql"}"#;
        let plan: CachedPlan = serde_json::from_str(json_str).expect("Failed to deserialize");

        assert_eq!(plan.engine, "postgresql");
        assert_eq!(plan.plan["node"], "SeqScan");
    }

    #[test]
    fn test_cached_plan_complex_structure() {
        let plan = CachedPlan {
            plan: json!({
                "Plan": {
                    "Node Type": "Hash Join",
                    "Plans": [
                        {"Node Type": "Seq Scan", "Relation Name": "users"},
                        {"Node Type": "Hash", "Plans": [{"Node Type": "Seq Scan", "Relation Name": "orders"}]}
                    ]
                }
            }),
            engine: "postgresql".to_string(),
        };

        let json_str = serde_json::to_string(&plan).expect("Failed to serialize");
        let deserialized: CachedPlan = serde_json::from_str(&json_str).expect("Failed to deserialize");

        assert_eq!(deserialized.engine, plan.engine);
        assert_eq!(deserialized.plan, plan.plan);
    }

    #[test]
    fn test_cached_plan_with_null_values() {
        let plan = CachedPlan {
            plan: json!({
                "filter": null,
                "cost": 10.5
            }),
            engine: "sqlite".to_string(),
        };

        let json_str = serde_json::to_string(&plan).expect("Failed to serialize");
        let deserialized: CachedPlan = serde_json::from_str(&json_str).expect("Failed to deserialize");

        assert_eq!(deserialized.plan["filter"], json!(null));
    }

    #[test]
    fn test_cached_plan_with_arrays() {
        let plan = CachedPlan {
            plan: json!([
                {"id": 1, "type": "PRIMARY"},
                {"id": 2, "type": "DERIVED"}
            ]),
            engine: "mysql".to_string(),
        };

        let json_str = serde_json::to_string(&plan).expect("Failed to serialize");
        let deserialized: CachedPlan = serde_json::from_str(&json_str).expect("Failed to deserialize");

        assert!(deserialized.plan.is_array());
        assert_eq!(deserialized.engine, "mysql");
    }

    #[test]
    fn test_cached_plan_clone() {
        let plan = CachedPlan {
            plan: json!({"node": "test"}),
            engine: "sqlite".to_string(),
        };

        let cloned = plan.clone();
        assert_eq!(cloned.engine, plan.engine);
        assert_eq!(cloned.plan, plan.plan);
    }
}

// Integration tests require Redis connection
#[cfg(all(test, feature = "integration-tests"))]
mod integration_tests {
    use redis::aio::ConnectionManager;
    use serde_json::json;

    use crate::cache::{cache_plan, get_cached_plan};

    async fn create_redis_connection() -> ConnectionManager {
        let client = redis::Client::open("redis://127.0.0.1:6379")
            .expect("Failed to create Redis client");
        ConnectionManager::new(client)
            .await
            .expect("Failed to connect to Redis")
    }

    async fn clear_redis(redis: &mut ConnectionManager) {
        use redis::AsyncCommands;
        let keys: Vec<String> = redis.keys("explain:*").await.expect("Failed to get keys");
        if !keys.is_empty() {
            let _: () = redis.del(keys).await.expect("Failed to delete keys");
        }
    }

    #[tokio::test]
    async fn test_cache_miss_returns_none() {
        let mut redis = create_redis_connection().await;
        clear_redis(&mut redis).await;

        let result = get_cached_plan(&mut redis, "SELECT * FROM nonexistent", "postgresql", false)
            .await
            .expect("Cache lookup failed");

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_cache_hit_returns_plan() {
        let mut redis = create_redis_connection().await;
        clear_redis(&mut redis).await;

        let sql = "SELECT * FROM users WHERE id = 1";
        let engine = "postgresql";
        let plan = json!({"Plan": {"Node Type": "Seq Scan"}});

        cache_plan(&mut redis, sql, engine, false, &plan)
            .await
            .expect("Failed to cache plan");

        let cached = get_cached_plan(&mut redis, sql, engine, false)
            .await
            .expect("Cache lookup failed")
            .expect("Expected cache hit");

        assert_eq!(cached.engine, engine);
        assert_eq!(cached.plan, plan);
    }

    #[tokio::test]
    async fn test_different_sql_different_keys() {
        let mut redis = create_redis_connection().await;
        clear_redis(&mut redis).await;

        let sql1 = "SELECT * FROM users WHERE id = 1";
        let sql2 = "SELECT * FROM users WHERE id = 2";
        let engine = "sqlite";
        let plan1 = json!({"plan": "plan1"});
        let plan2 = json!({"plan": "plan2"});

        cache_plan(&mut redis, sql1, engine, false, &plan1)
            .await
            .expect("Failed to cache plan 1");
        cache_plan(&mut redis, sql2, engine, false, &plan2)
            .await
            .expect("Failed to cache plan 2");

        let cached1 = get_cached_plan(&mut redis, sql1, engine, false)
            .await
            .expect("Cache lookup failed")
            .expect("Expected cache hit for sql1");

        let cached2 = get_cached_plan(&mut redis, sql2, engine, false)
            .await
            .expect("Cache lookup failed")
            .expect("Expected cache hit for sql2");

        assert_eq!(cached1.plan, plan1);
        assert_eq!(cached2.plan, plan2);
    }

    #[tokio::test]
    async fn test_analyze_flag_affects_cache_key() {
        let mut redis = create_redis_connection().await;
        clear_redis(&mut redis).await;

        let sql = "SELECT * FROM users";
        let engine = "postgresql";
        let plan_no_analyze = json!({"analyze": false});
        let plan_analyze = json!({"analyze": true});

        cache_plan(&mut redis, sql, engine, false, &plan_no_analyze)
            .await
            .expect("Failed to cache plan without analyze");
        cache_plan(&mut redis, sql, engine, true, &plan_analyze)
            .await
            .expect("Failed to cache plan with analyze");

        let cached_no_analyze = get_cached_plan(&mut redis, sql, engine, false)
            .await
            .expect("Cache lookup failed")
            .expect("Expected cache hit for analyze=false");

        let cached_analyze = get_cached_plan(&mut redis, sql, engine, true)
            .await
            .expect("Cache lookup failed")
            .expect("Expected cache hit for analyze=true");

        assert_eq!(cached_no_analyze.plan, plan_no_analyze);
        assert_eq!(cached_analyze.plan, plan_analyze);
    }

    #[tokio::test]
    async fn test_concurrent_cache_writes() {
        let mut redis = create_redis_connection().await;
        clear_redis(&mut redis).await;

        let mut handles = vec![];

        for i in 0..10 {
            let mut redis_clone = redis.clone();
            let handle = tokio::spawn(async move {
                let sql = format!("SELECT * FROM users WHERE id = {}", i);
                let engine = "sqlite";
                let plan = json!({"id": i});

                cache_plan(&mut redis_clone, &sql, engine, false, &plan)
                    .await
                    .expect("Failed to cache plan");
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await.expect("Task failed");
        }

        // Verify all entries were cached
        for i in 0..10 {
            let sql = format!("SELECT * FROM users WHERE id = {}", i);
            let cached = get_cached_plan(&mut redis, &sql, "sqlite", false)
                .await
                .expect("Cache lookup failed")
                .expect("Expected cache hit");

            assert_eq!(cached.plan, json!({"id": i}));
        }
    }
}
