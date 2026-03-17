//! POST /api/share - URL shortening for shared queries.

use std::collections::HashMap;
use std::sync::Mutex;

use rocket::serde::json::Json;
use rocket::State;
use serde::{Deserialize, Serialize};

use crate::errors::{ApiResult, AppError};

/// In-memory store for shared queries.
#[derive(Debug, Default)]
pub struct ShareStore {
    entries: Mutex<HashMap<String, ShareEntry>>,
    counter: Mutex<u64>,
}

/// A stored shared query.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ShareEntry {
    sql: String,
    engine: Option<String>,
    dialect_from: Option<String>,
    dialect_to: Option<String>,
}

impl ShareStore {
    /// Create a new empty share store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn next_id(&self) -> String {
        let mut counter =
            self.counter.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        *counter += 1;
        format!("{:x}", *counter)
    }
}

/// Request body for creating a shared link.
#[derive(Debug, Deserialize)]
pub struct CreateShareRequest {
    /// SQL to share.
    pub sql: String,
    /// Optional engine name.
    pub engine: Option<String>,
    /// Optional source dialect.
    pub dialect_from: Option<String>,
    /// Optional target dialect.
    pub dialect_to: Option<String>,
}

/// Response body from creating a shared link.
#[derive(Debug, Serialize)]
pub struct CreateShareResponse {
    /// The short ID for this share.
    pub id: String,
}

/// Response body from retrieving a shared query.
#[derive(Debug, Serialize)]
pub struct GetShareResponse {
    /// The shared SQL.
    pub sql: String,
    /// Optional engine.
    pub engine: Option<String>,
    /// Optional source dialect.
    pub dialect_from: Option<String>,
    /// Optional target dialect.
    pub dialect_to: Option<String>,
}

/// Create a shared link for a query.
#[allow(clippy::needless_pass_by_value)]
#[rocket::post("/api/share", data = "<req>")]
pub fn create_share(
    req: Json<CreateShareRequest>,
    store: &State<ShareStore>,
) -> ApiResult<CreateShareResponse> {
    if req.sql.trim().is_empty() {
        return Err(AppError::bad_request(
            "empty_sql",
            "SQL cannot be empty",
        ));
    }

    let id = store.next_id();
    let entry = ShareEntry {
        sql: req.sql.clone(),
        engine: req.engine.clone(),
        dialect_from: req.dialect_from.clone(),
        dialect_to: req.dialect_to.clone(),
    };

    let mut entries =
        store.entries.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    entries.insert(id.clone(), entry);

    Ok(Json(CreateShareResponse { id }))
}

/// Retrieve a shared query by ID.
#[rocket::get("/api/share/<id>")]
pub fn get_share(
    id: &str,
    store: &State<ShareStore>,
) -> ApiResult<GetShareResponse> {
    let entries =
        store.entries.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

    let entry = entries.get(id).ok_or_else(|| {
        AppError::not_found(format!("share '{id}' not found"))
    })?;

    Ok(Json(GetShareResponse {
        sql: entry.sql.clone(),
        engine: entry.engine.clone(),
        dialect_from: entry.dialect_from.clone(),
        dialect_to: entry.dialect_to.clone(),
    }))
}
