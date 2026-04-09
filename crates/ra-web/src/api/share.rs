//! POST /api/share - URL shortening for shared queries with Redis backend.

use rand::Rng;
use redis::{aio::ConnectionManager, AsyncCommands};
use rocket::serde::json::Json;
use rocket::State;
use serde::{Deserialize, Serialize};

use crate::errors::{ApiResult, AppError};

const SHARE_TTL: u64 = 86400;
const SHARE_KEY_PREFIX: &str = "share:";
const BASE62_CHARS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Generate a base62-encoded random ID.
fn generate_id() -> String {
    let mut rng = rand::thread_rng();
    let mut id = String::with_capacity(8);
    for _ in 0..8 {
        let idx = rng.gen_range(0..BASE62_CHARS.len());
        id.push(BASE62_CHARS[idx] as char);
    }
    id
}

/// Panel state for the visualization UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelState {
    pub id: String,
    pub visible: bool,
    pub position: Option<PanelPosition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelPosition {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A stored shared query.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ShareEntry {
    sql: String,
    panels: Vec<PanelState>,
}

/// Request body for creating a shared link.
#[derive(Debug, Deserialize)]
pub struct CreateShareRequest {
    pub sql: String,
    #[serde(default)]
    pub panels: Vec<PanelState>,
}

/// Response body from creating a shared link.
#[derive(Debug, Serialize)]
pub struct CreateShareResponse {
    pub id: String,
    pub url: String,
}

/// Response body from retrieving a shared query.
#[derive(Debug, Serialize)]
pub struct GetShareResponse {
    pub sql: String,
    pub panels: Vec<PanelState>,
}

/// Create a shared link for a query.
#[allow(clippy::needless_pass_by_value)]
#[rocket::post("/api/share", data = "<req>")]
pub async fn create_share(
    req: Json<CreateShareRequest>,
    redis: &State<ConnectionManager>,
) -> ApiResult<CreateShareResponse> {
    if req.sql.trim().is_empty() {
        return Err(AppError::bad_request(
            "empty_sql",
            "SQL cannot be empty",
        ));
    }

    let id = generate_id();
    let entry = ShareEntry {
        sql: req.sql.clone(),
        panels: req.panels.clone(),
    };

    let json = serde_json::to_string(&entry).map_err(|e| {
        tracing::error!("Failed to serialize share entry: {e}");
        AppError::internal(format!("Failed to serialize share entry: {e}"))
    })?;

    let key = format!("{SHARE_KEY_PREFIX}{id}");
    let mut conn = redis.inner().clone();

    conn.set_ex::<_, _, ()>(&key, json, SHARE_TTL)
        .await
        .map_err(|e| {
            tracing::error!("Redis error during share creation: {e}");
            AppError::internal(format!("Failed to store share: {e}"))
        })?;

    tracing::info!("Created share with id={id}, ttl={SHARE_TTL}s");

    Ok(Json(CreateShareResponse {
        id: id.clone(),
        url: format!("/share/{id}"),
    }))
}

/// Retrieve a shared query by ID.
#[rocket::get("/api/share/<id>")]
pub async fn get_share(
    id: &str,
    redis: &State<ConnectionManager>,
) -> ApiResult<GetShareResponse> {
    let key = format!("{SHARE_KEY_PREFIX}{id}");
    let mut conn = redis.inner().clone();

    let json: Option<String> = conn.get(&key).await.map_err(|e| {
        tracing::error!("Redis error during share retrieval: {e}");
        AppError::internal(format!("Failed to retrieve share: {e}"))
    })?;

    let json = json.ok_or_else(|| {
        tracing::warn!("Share not found: {id}");
        AppError::not_found(format!("share '{id}' not found"))
    })?;

    let entry: ShareEntry = serde_json::from_str(&json).map_err(|e| {
        tracing::error!("Failed to deserialize share entry: {e}");
        AppError::internal(format!("Failed to deserialize share: {e}"))
    })?;

    tracing::info!("Retrieved share with id={id}");

    Ok(Json(GetShareResponse {
        sql: entry.sql,
        panels: entry.panels,
    }))
}
