//! Storage backends for WASM database persistence.
//!
//! Browsers offer two persistence mechanisms for WASM databases:
//! - **OPFS** (Origin Private File System): high-performance file
//!   storage with synchronous access via `FileSystemSyncAccessHandle`.
//! - **`IndexedDB`**: key-value storage available in all modern
//!   browsers, used as a fallback when OPFS is unavailable.

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::Window;

use crate::errors::{Result, WasmDbError};

/// Which persistence mechanism to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StorageBackend {
    /// In-memory only; data lost on page unload.
    Memory,
    /// Origin Private File System (fast, file-like access).
    Opfs,
    /// `IndexedDB` (broad compatibility, async key-value).
    IndexedDb,
}

impl std::fmt::Display for StorageBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Memory => write!(f, "memory"),
            Self::Opfs => write!(f, "opfs"),
            Self::IndexedDb => write!(f, "indexeddb"),
        }
    }
}

/// Probe whether OPFS is available in the current browsing context.
///
/// OPFS requires a secure context (HTTPS or localhost) and
/// cross-origin isolation headers.
#[must_use]
pub fn opfs_available() -> bool {
    global_window()
        .and_then(|w| {
            let sm = w.navigator().storage();
            js_sys::Reflect::get(&sm, &JsValue::from_str("getDirectory")).ok()
        })
        .is_some_and(|val| val.is_function())
}

/// Probe whether `IndexedDB` is available.
#[must_use]
pub fn indexeddb_available() -> bool {
    global_window().is_some_and(|w| w.indexed_db().ok().flatten().is_some())
}

/// Choose the best available storage backend, falling back
/// from OPFS to `IndexedDB` to Memory.
#[must_use]
pub fn best_available_backend() -> StorageBackend {
    if opfs_available() {
        return StorageBackend::Opfs;
    }
    if indexeddb_available() {
        return StorageBackend::IndexedDb;
    }
    StorageBackend::Memory
}

/// Validate that the requested storage backend is actually
/// available in the current environment.
///
/// # Errors
///
/// Returns an error if the backend is not available.
pub fn validate_backend(backend: StorageBackend) -> Result<()> {
    match backend {
        StorageBackend::Memory => Ok(()),
        StorageBackend::Opfs => {
            if opfs_available() {
                Ok(())
            } else {
                Err(WasmDbError::Storage(
                    "OPFS is not available; ensure secure context \
                     (HTTPS) and cross-origin isolation headers \
                     (COOP/COEP) are configured"
                        .into(),
                ))
            }
        }
        StorageBackend::IndexedDb => {
            if indexeddb_available() {
                Ok(())
            } else {
                Err(WasmDbError::Storage(
                    "IndexedDB is not available in this context".into(),
                ))
            }
        }
    }
}

/// Delete all data for a named database from the given storage
/// backend. Useful for cleanup and testing.
///
/// # Errors
///
/// Returns an error if deletion fails.
pub fn delete_database(name: &str, backend: StorageBackend) -> Result<()> {
    match backend {
        StorageBackend::Memory => Ok(()),
        StorageBackend::Opfs => {
            tracing::debug!("OPFS deletion for '{}' not yet wired", name);
            Ok(())
        }
        StorageBackend::IndexedDb => {
            let window = global_window()
                .ok_or_else(|| WasmDbError::Storage("no global window object".into()))?;
            let idb = window
                .indexed_db()
                .map_err(WasmDbError::from)?
                .ok_or_else(|| WasmDbError::Storage("IndexedDB not available".into()))?;
            idb.delete_database(name).map_err(WasmDbError::from)?;
            Ok(())
        }
    }
}

/// Cross-origin isolation info for diagnosing OPFS availability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossOriginStatus {
    /// Whether the page reports itself as cross-origin isolated.
    pub is_isolated: bool,
    /// The value of `crossOriginIsolated` on the global scope.
    pub raw_value: Option<String>,
}

/// Check cross-origin isolation status of the current context.
#[must_use]
pub fn cross_origin_status() -> CrossOriginStatus {
    let isolated =
        js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("crossOriginIsolated"))
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

    CrossOriginStatus {
        is_isolated: isolated,
        raw_value: if isolated {
            Some("true".into())
        } else {
            Some("false".into())
        },
    }
}

fn global_window() -> Option<Window> {
    js_sys::global().dyn_into::<Window>().ok()
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn storage_backend_display() {
        assert_eq!(StorageBackend::Memory.to_string(), "memory");
        assert_eq!(StorageBackend::Opfs.to_string(), "opfs");
        assert_eq!(StorageBackend::IndexedDb.to_string(), "indexeddb");
    }

    #[test]
    fn storage_backend_serde_roundtrip() {
        for backend in [
            StorageBackend::Memory,
            StorageBackend::Opfs,
            StorageBackend::IndexedDb,
        ] {
            let json = serde_json::to_string(&backend).expect("serialize");
            let out: StorageBackend = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(backend, out);
        }
    }

    #[test]
    fn validate_memory_always_ok() {
        assert!(validate_backend(StorageBackend::Memory).is_ok());
    }

    #[test]
    fn cross_origin_status_fields() {
        let status = CrossOriginStatus {
            is_isolated: false,
            raw_value: Some("false".into()),
        };
        assert!(!status.is_isolated);
        assert_eq!(status.raw_value.as_deref(), Some("false"));
    }

    #[test]
    fn cross_origin_status_serde_roundtrip() {
        let status = CrossOriginStatus {
            is_isolated: true,
            raw_value: Some("true".into()),
        };
        let json = serde_json::to_string(&status).expect("serialize");
        let out: CrossOriginStatus = serde_json::from_str(&json).expect("deserialize");
        assert!(out.is_isolated);
    }
}
