//! Unified error handling for API responses.

use rocket::http::Status;
use rocket::response::Responder;
use rocket::serde::json::Json;
use serde::Serialize;

/// Standard API error response body.
#[derive(Debug, Serialize)]
pub struct ApiError {
    /// Machine-readable error code.
    pub code: String,
    /// Human-readable message.
    pub message: String,
}

/// Wrapper that converts domain errors into proper HTTP responses.
#[derive(Debug)]
pub struct AppError {
    pub status: Status,
    pub body: ApiError,
}

impl AppError {
    pub fn bad_request(code: &str, message: impl Into<String>) -> Self {
        Self {
            status: Status::BadRequest,
            body: ApiError {
                code: code.to_owned(),
                message: message.into(),
            },
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            status: Status::InternalServerError,
            body: ApiError {
                code: "internal_error".to_owned(),
                message: message.into(),
            },
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: Status::NotFound,
            body: ApiError {
                code: "not_found".to_owned(),
                message: message.into(),
            },
        }
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self {
            status: Status::RequestTimeout,
            body: ApiError {
                code: "query_timeout".to_owned(),
                message: message.into(),
            },
        }
    }
}

impl<'r> Responder<'r, 'static> for AppError {
    fn respond_to(
        self,
        req: &'r rocket::Request<'_>,
    ) -> rocket::response::Result<'static> {
        let json = Json(self.body);
        rocket::Response::build_from(json.respond_to(req)?)
            .status(self.status)
            .ok()
    }
}

/// Convenience result alias for API handlers.
pub type ApiResult<T> = Result<Json<T>, AppError>;
