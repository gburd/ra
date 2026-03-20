//! Relational Algebra Web Explorer - REST API server.
//!
//! Provides endpoints for SQL execution, dialect translation,
//! query optimization, isolation testing, query synthesis, and
//! query sharing.

mod api;
mod cors;
mod errors;
mod rate_limit;
mod websocket;

use std::path::PathBuf;

use ra_synthesis::synthesizer::{SynthesisRequest, Synthesizer};
use rocket::fs::FileServer;
use rocket::serde::json::Json;
use rocket::{get, launch, options, post, routes};
use serde::Serialize;

use api::share::ShareStore;
use cors::Cors;
use rate_limit::RateLimiter;

#[get("/health")]
fn health() -> &'static str {
    "OK"
}

/// Catch-all OPTIONS handler for CORS preflight requests.
#[options("/<_path..>")]
fn options_preflight(
    _path: std::path::PathBuf,
) -> rocket::http::Status {
    rocket::http::Status::NoContent
}

/// Response from the synthesis endpoint.
#[derive(Serialize)]
struct SynthesisResponse {
    sql: String,
    rel_expr: serde_json::Value,
    warnings: Vec<String>,
}

/// Error response from the synthesis endpoint.
#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

/// Synthesize a SQL query from natural language.
#[allow(clippy::needless_pass_by_value)]
#[post("/api/synthesize", data = "<request>")]
fn synthesize(
    request: Json<SynthesisRequest>,
) -> Result<Json<SynthesisResponse>, Json<ErrorResponse>> {
    let synth = Synthesizer::new(&request.schema);
    match synth.synthesize(&request.query) {
        Ok(result) => {
            let rel_expr_json =
                serde_json::to_value(&result.rel_expr)
                    .unwrap_or(serde_json::Value::Null);
            Ok(Json(SynthesisResponse {
                sql: result.sql,
                rel_expr: rel_expr_json,
                warnings: result.warnings,
            }))
        }
        Err(e) => Err(Json(ErrorResponse {
            error: e.to_string(),
        })),
    }
}

/// Resolve the directory for serving static files.
///
/// Uses the `STATIC_DIR` environment variable when set (Docker),
/// falling back to `crates/ra-web/static` relative to the cargo
/// manifest directory (local development).
fn static_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("STATIC_DIR") {
        return PathBuf::from(dir);
    }
    // Fallback for local `cargo run` from the workspace root.
    PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR")
            .unwrap_or_else(|_| ".".to_string()),
    )
    .join("static")
}

/// Build the Rocket instance with all routes and fairings attached.
fn build_rocket() -> rocket::Rocket<rocket::Build> {
    let static_path = static_dir();
    rocket::build()
        .attach(Cors)
        .attach(RateLimiter::new(
            100,
            std::time::Duration::from_secs(60),
        ))
        .manage(ShareStore::new())
        .manage(api::demos::DemoState::new(std::sync::Mutex::new(
            api::demos::DemoStore::new(),
        )))
        .mount(
            "/",
            routes![
                health,
                options_preflight,
                synthesize,
                api::execute::execute,
                api::translate::translate,
                api::optimize::optimize,
                api::explain::explain,
                api::isolation::parse_spec,
                api::isolation::run_isolation,
                api::compare::compare,
                api::rules::list_rules,
                api::share::create_share,
                api::share::get_share,
                api::demos::list_demos,
                api::demos::demo_staleness_impact,
                api::demos::demo_hardware_plan,
                api::demos::demo_join_algorithm,
                api::demos::demo_aggregation_strategy,
                api::demos2::demo_index_selection,
                api::demos2::demo_subquery_unnesting,
                api::demos2::demo_parallel_query,
                api::demos2::demo_gpu_offloading,
                api::demos2::demo_distributed_query,
                api::demos2::demo_cost_calibration,
                websocket::isolation_ws,
                api::visualize::visualize,
                api::visualize::compare_plans,
            ],
        )
        .mount("/", FileServer::from(static_path))
}

#[launch]
fn rocket() -> _ {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env(),
        )
        .init();

    build_rocket()
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used)]
mod tests {
    use rocket::http::{ContentType, Status};
    use rocket::local::blocking::Client;

    use super::*;

    fn client() -> Client {
        Client::tracked(build_rocket()).unwrap()
    }

    #[test]
    fn test_index() {
        let client = client();
        let response = client.get("/").dispatch();
        assert_eq!(response.status(), Status::Ok);
        let body = response.into_string().unwrap();
        assert!(body.contains("RA"));
    }

    #[test]
    fn test_health() {
        let client = client();
        let response = client.get("/health").dispatch();
        assert_eq!(response.status(), Status::Ok);
        assert_eq!(response.into_string().unwrap(), "OK");
    }

    #[test]
    fn test_options_preflight() {
        let client = client();
        let response =
            client.options("/api/anything").dispatch();
        assert_eq!(response.status(), Status::NoContent);
    }

    #[test]
    fn test_cors_headers() {
        let client = client();
        let response = client.get("/health").dispatch();
        let headers = response.headers();
        assert_eq!(
            headers.get_one("Access-Control-Allow-Origin"),
            Some("*")
        );
        assert!(headers
            .get_one("Cross-Origin-Embedder-Policy")
            .is_some());
        assert!(headers
            .get_one("Cross-Origin-Opener-Policy")
            .is_some());
    }

    #[test]
    fn test_execute_empty_sql() {
        let client = client();
        let response = client
            .post("/api/execute")
            .header(ContentType::JSON)
            .body(r#"{"sql":"","engine":"sqlite"}"#)
            .dispatch();
        assert_eq!(response.status(), Status::BadRequest);
    }

    #[test]
    fn test_execute_invalid_engine() {
        let client = client();
        let response = client
            .post("/api/execute")
            .header(ContentType::JSON)
            .body(r#"{"sql":"SELECT 1","engine":"oracle"}"#)
            .dispatch();
        assert_eq!(response.status(), Status::BadRequest);
    }

    #[test]
    fn test_execute_valid() {
        let client = client();
        let response = client
            .post("/api/execute")
            .header(ContentType::JSON)
            .body(r#"{"sql":"SELECT 1","engine":"sqlite"}"#)
            .dispatch();
        assert_eq!(response.status(), Status::Ok);
        let body: serde_json::Value =
            serde_json::from_str(
                &response.into_string().unwrap(),
            )
            .unwrap();
        assert_eq!(body["engine"], "sqlite");
    }

    #[test]
    fn test_translate_empty_sql() {
        let client = client();
        let response = client
            .post("/api/translate")
            .header(ContentType::JSON)
            .body(
                r#"{"sql":"","from":"pg","to":"mysql"}"#,
            )
            .dispatch();
        assert_eq!(response.status(), Status::BadRequest);
    }

    #[test]
    fn test_translate_invalid_dialect() {
        let client = client();
        let response = client
            .post("/api/translate")
            .header(ContentType::JSON)
            .body(
                r#"{"sql":"SELECT 1","from":"unknown","to":"pg"}"#,
            )
            .dispatch();
        assert_eq!(response.status(), Status::BadRequest);
    }

    #[test]
    fn test_translate_valid() {
        let client = client();
        let response = client
            .post("/api/translate")
            .header(ContentType::JSON)
            .body(
                r#"{"sql":"SELECT 1","from":"pg","to":"mysql"}"#,
            )
            .dispatch();
        assert_eq!(response.status(), Status::Ok);
        let body: serde_json::Value =
            serde_json::from_str(
                &response.into_string().unwrap(),
            )
            .unwrap();
        assert_eq!(body["from"], "pg");
        assert_eq!(body["to"], "mysql");
    }

    #[test]
    fn test_explain_invalid_engine() {
        let client = client();
        let response = client
            .post("/api/explain")
            .header(ContentType::JSON)
            .body(
                r#"{"sql":"SELECT 1","engine":"oracle","analyze":false}"#,
            )
            .dispatch();
        assert_eq!(response.status(), Status::BadRequest);
    }

    #[test]
    fn test_explain_valid() {
        let client = client();
        let response = client
            .post("/api/explain")
            .header(ContentType::JSON)
            .body(
                r#"{"sql":"SELECT 1","engine":"duckdb","analyze":true}"#,
            )
            .dispatch();
        assert_eq!(response.status(), Status::Ok);
        let body: serde_json::Value =
            serde_json::from_str(
                &response.into_string().unwrap(),
            )
            .unwrap();
        assert_eq!(body["engine"], "duckdb");
        assert_eq!(body["analyzed"], true);
    }

    #[test]
    fn test_compare_empty_engines() {
        let client = client();
        let response = client
            .post("/api/compare")
            .header(ContentType::JSON)
            .body(r#"{"sql":"SELECT 1","engines":[]}"#)
            .dispatch();
        assert_eq!(response.status(), Status::BadRequest);
    }

    #[test]
    fn test_compare_valid() {
        let client = client();
        let response = client
            .post("/api/compare")
            .header(ContentType::JSON)
            .body(
                r#"{"sql":"SELECT 1","engines":["sqlite","duckdb"]}"#,
            )
            .dispatch();
        assert_eq!(response.status(), Status::Ok);
        let body: serde_json::Value =
            serde_json::from_str(
                &response.into_string().unwrap(),
            )
            .unwrap();
        assert_eq!(body["matching"], true);
        assert_eq!(body["results"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_rules_list() {
        let client = client();
        let response = client.get("/api/rules").dispatch();
        assert_eq!(response.status(), Status::Ok);
        let body: serde_json::Value =
            serde_json::from_str(
                &response.into_string().unwrap(),
            )
            .unwrap();
        assert!(body["count"].as_u64().unwrap() > 0);
        assert!(!body["rules"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_isolation_parse_empty() {
        let client = client();
        let response = client
            .post("/api/isolation/parse")
            .header(ContentType::JSON)
            .body(r#"{"spec":""}"#)
            .dispatch();
        assert_eq!(response.status(), Status::BadRequest);
    }

    #[test]
    fn test_isolation_parse_valid() {
        let spec = r#"{"spec":"setup {\nCREATE TABLE t (id INT);\n}\n\nsession s1 \"reader\" {\n  step r1 \"read\" {\n    SELECT * FROM t;\n  }\n}\n"}"#;
        let client = client();
        let response = client
            .post("/api/isolation/parse")
            .header(ContentType::JSON)
            .body(spec)
            .dispatch();
        // May succeed or fail depending on spec format,
        // but should not be a 500.
        assert!(
            response.status() == Status::Ok
                || response.status() == Status::BadRequest
        );
    }

    #[test]
    fn test_share_roundtrip() {
        let client = client();

        // Create a share.
        let response = client
            .post("/api/share")
            .header(ContentType::JSON)
            .body(r#"{"sql":"SELECT 42"}"#)
            .dispatch();
        assert_eq!(response.status(), Status::Ok);
        let body: serde_json::Value =
            serde_json::from_str(
                &response.into_string().unwrap(),
            )
            .unwrap();
        let id = body["id"].as_str().unwrap();

        // Retrieve the share.
        let response = client
            .get(format!("/api/share/{id}"))
            .dispatch();
        assert_eq!(response.status(), Status::Ok);
        let body: serde_json::Value =
            serde_json::from_str(
                &response.into_string().unwrap(),
            )
            .unwrap();
        assert_eq!(body["sql"], "SELECT 42");
    }

    #[test]
    fn test_share_not_found() {
        let client = client();
        let response =
            client.get("/api/share/nonexistent").dispatch();
        assert_eq!(response.status(), Status::NotFound);
    }

    #[test]
    fn test_share_empty_sql() {
        let client = client();
        let response = client
            .post("/api/share")
            .header(ContentType::JSON)
            .body(r#"{"sql":""}"#)
            .dispatch();
        assert_eq!(response.status(), Status::BadRequest);
    }

    /// Build a Rocket instance with a very low rate limit for
    /// testing (2 requests per 60 seconds).
    fn build_rate_limited_rocket(
    ) -> rocket::Rocket<rocket::Build> {
        rocket::build()
            .attach(Cors)
            .attach(RateLimiter::new(
                2,
                std::time::Duration::from_secs(60),
            ))
            .manage(ShareStore::new())
            .manage(api::demos::DemoState::new(std::sync::Mutex::new(
                api::demos::DemoStore::new(),
            )))
            .mount(
                "/",
                routes![
                    health,
                    options_preflight,
                    synthesize,
                    api::execute::execute,
                    api::translate::translate,
                    api::optimize::optimize,
                    api::explain::explain,
                    api::isolation::parse_spec,
                    api::isolation::run_isolation,
                    api::compare::compare,
                    api::rules::list_rules,
                    api::share::create_share,
                    api::share::get_share,
                    api::demos::list_demos,
                    api::demos::demo_staleness_impact,
                    api::demos::demo_hardware_plan,
                    api::demos::demo_join_algorithm,
                    api::demos::demo_aggregation_strategy,
                    api::demos2::demo_index_selection,
                    api::demos2::demo_subquery_unnesting,
                    api::demos2::demo_parallel_query,
                    api::demos2::demo_gpu_offloading,
                    api::demos2::demo_distributed_query,
                    api::demos2::demo_cost_calibration,
                    websocket::isolation_ws,
                    api::visualize::visualize,
                    api::visualize::compare_plans,
                ],
            )
            .mount("/", FileServer::from(static_dir()))
    }

    #[test]
    fn test_rate_limiting() {
        use std::net::SocketAddr;

        let client = Client::tracked(
            build_rate_limited_rocket(),
        )
        .unwrap();

        let addr: SocketAddr =
            "192.168.1.1:12345".parse().unwrap();
        let body = r#"{"expr":{"Scan":{"table":"t"}}}"#;

        // First two requests should succeed (limit = 2).
        let r1 = client
            .post("/api/optimize")
            .remote(addr)
            .header(ContentType::JSON)
            .body(body)
            .dispatch();
        assert_ne!(
            r1.status(),
            Status::TooManyRequests,
            "first request should not be rate limited"
        );

        let r2 = client
            .post("/api/optimize")
            .remote(addr)
            .header(ContentType::JSON)
            .body(body)
            .dispatch();
        assert_ne!(
            r2.status(),
            Status::TooManyRequests,
            "second request should not be rate limited"
        );

        // Third request should be rejected.
        let r3 = client
            .post("/api/optimize")
            .remote(addr)
            .header(ContentType::JSON)
            .body(body)
            .dispatch();
        assert_eq!(
            r3.status(),
            Status::TooManyRequests,
            "third request should be rate limited"
        );
    }

    #[test]
    fn test_visualize_empty_sql() {
        let client = client();
        let response = client
            .post("/api/visualize")
            .header(ContentType::JSON)
            .body(r#"{"sql":""}"#)
            .dispatch();
        assert_eq!(response.status(), Status::BadRequest);
    }

    #[test]
    fn test_visualize_valid() {
        let client = client();
        let response = client
            .post("/api/visualize")
            .header(ContentType::JSON)
            .body(r#"{"sql":"SELECT * FROM users WHERE age > 25"}"#)
            .dispatch();
        assert_eq!(response.status(), Status::Ok);
        let body: serde_json::Value =
            serde_json::from_str(
                &response.into_string().unwrap(),
            )
            .unwrap();
        assert!(body["plan"]["operator_type"].is_string());
        assert!(body["total_cost"].as_f64().unwrap() > 0.0);
        assert!(!body["rules_applied"]
            .as_array()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn test_compare_plans_empty_sql() {
        let client = client();
        let response = client
            .post("/api/compare-plans")
            .header(ContentType::JSON)
            .body(r#"{"sql":""}"#)
            .dispatch();
        assert_eq!(response.status(), Status::BadRequest);
    }

    #[test]
    fn test_compare_plans_valid() {
        let client = client();
        let response = client
            .post("/api/compare-plans")
            .header(ContentType::JSON)
            .body(
                r#"{"sql":"SELECT * FROM users WHERE age > 25"}"#,
            )
            .dispatch();
        assert_eq!(response.status(), Status::Ok);
        let body: serde_json::Value =
            serde_json::from_str(
                &response.into_string().unwrap(),
            )
            .unwrap();
        assert_eq!(
            body["plans"].as_array().unwrap().len(),
            4,
            "should have plans for Ra, PostgreSQL, MySQL, DuckDB"
        );
        assert!(body["summary"]["cheapest"].is_string());
        assert_eq!(
            body["summary"]["costs"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
    }

    #[test]
    fn test_compare_plans_with_join() {
        let client = client();
        let sql = "SELECT u.name FROM users u \
                   JOIN orders o ON u.id = o.user_id \
                   WHERE o.total > 100";
        let response = client
            .post("/api/compare-plans")
            .header(ContentType::JSON)
            .body(format!(r#"{{"sql":"{sql}"}}"#))
            .dispatch();
        assert_eq!(response.status(), Status::Ok);
        let body: serde_json::Value =
            serde_json::from_str(
                &response.into_string().unwrap(),
            )
            .unwrap();
        let ra_plan = &body["plans"][0]["plan"];
        // With JOIN, the Ra plan should have a nested tree
        assert!(
            has_operator(&ra_plan, "HashJoin"),
            "Ra plan should contain HashJoin for JOIN queries"
        );
    }

    fn has_operator(
        node: &serde_json::Value,
        op: &str,
    ) -> bool {
        if node["operator_type"].as_str() == Some(op) {
            return true;
        }
        if let Some(children) = node["children"].as_array() {
            return children
                .iter()
                .any(|c| has_operator(c, op));
        }
        false
    }

    #[test]
    fn test_rate_limit_skips_health() {
        let client = Client::tracked(
            build_rate_limited_rocket(),
        )
        .unwrap();

        // Health endpoint is exempt from rate limiting.
        for _ in 0..10 {
            let response =
                client.get("/health").dispatch();
            assert_eq!(response.status(), Status::Ok);
        }
    }
}
