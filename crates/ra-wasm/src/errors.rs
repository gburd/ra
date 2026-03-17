//! Error types for WASM database adapters.

use wasm_bindgen::JsValue;

/// Errors that can occur during WASM database operations.
#[derive(Debug, thiserror::Error)]
pub enum WasmDbError {
    /// Database initialization failed.
    #[error("initialization failed: {0}")]
    Init(String),

    /// SQL query execution failed.
    #[error("query execution failed: {0}")]
    Query(String),

    /// Connection pool exhausted or connection unavailable.
    #[error("connection error: {0}")]
    Connection(String),

    /// Storage backend (OPFS or `IndexedDB`) operation failed.
    #[error("storage error: {0}")]
    Storage(String),

    /// JavaScript interop error.
    #[error("JS interop error: {0}")]
    JsError(String),

    /// Serialization or deserialization failed.
    #[error("serialization error: {0}")]
    Serialization(String),

    /// The requested database backend is not available.
    #[error("unsupported backend: {0}")]
    UnsupportedBackend(String),

    /// Operation timed out.
    #[error("operation timed out after {0}ms")]
    Timeout(u32),
}

impl From<JsValue> for WasmDbError {
    fn from(val: JsValue) -> Self {
        let msg = val
            .as_string()
            .or_else(|| {
                js_sys::JSON::stringify(&val)
                    .ok()
                    .and_then(|s| s.as_string())
            })
            .unwrap_or_else(|| format!("{val:?}"));
        Self::JsError(msg)
    }
}

impl From<serde_json::Error> for WasmDbError {
    fn from(err: serde_json::Error) -> Self {
        Self::Serialization(err.to_string())
    }
}

impl From<WasmDbError> for JsValue {
    fn from(err: WasmDbError) -> Self {
        JsValue::from_str(&err.to_string())
    }
}

/// Convenience alias for results using [`WasmDbError`].
pub type Result<T> = std::result::Result<T, WasmDbError>;

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn error_display_init() {
        let err = WasmDbError::Init("bad config".into());
        assert_eq!(err.to_string(), "initialization failed: bad config");
    }

    #[test]
    fn error_display_query() {
        let err = WasmDbError::Query("syntax error".into());
        assert_eq!(err.to_string(), "query execution failed: syntax error");
    }

    #[test]
    fn error_display_connection() {
        let err = WasmDbError::Connection("pool full".into());
        assert_eq!(err.to_string(), "connection error: pool full");
    }

    #[test]
    fn error_display_storage() {
        let err = WasmDbError::Storage("no opfs".into());
        assert_eq!(err.to_string(), "storage error: no opfs");
    }

    #[test]
    fn error_display_timeout() {
        let err = WasmDbError::Timeout(5000);
        assert_eq!(err.to_string(), "operation timed out after 5000ms");
    }

    #[test]
    fn error_display_unsupported() {
        let err = WasmDbError::UnsupportedBackend("MySQL".into());
        assert_eq!(err.to_string(), "unsupported backend: MySQL");
    }

    #[test]
    fn from_serde_error() {
        let serde_err = serde_json::from_str::<String>("{{").expect_err("should fail");
        let wasm_err = WasmDbError::from(serde_err);
        assert!(matches!(wasm_err, WasmDbError::Serialization(_)));
    }

    #[test]
    #[cfg(target_arch = "wasm32")]
    fn error_to_jsvalue() {
        let err = WasmDbError::Query("fail".into());
        let js: JsValue = err.into();
        assert_eq!(
            js.as_string().as_deref(),
            Some("query execution failed: fail")
        );
    }
}
