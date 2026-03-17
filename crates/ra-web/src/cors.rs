//! CORS fairing for cross-origin isolation headers.
//!
//! Configures the required headers for `SharedArrayBuffer` and
//! WASM threading support in browsers.

use rocket::fairing::{Fairing, Info, Kind};
use rocket::http::Header;
use rocket::{Request, Response};

/// Attaches CORS and cross-origin isolation headers to every response.
pub struct Cors;

#[rocket::async_trait]
impl Fairing for Cors {
    fn info(&self) -> Info {
        Info {
            name: "CORS + Cross-Origin Isolation",
            kind: Kind::Response,
        }
    }

    async fn on_response<'r>(
        &self,
        _request: &'r Request<'_>,
        response: &mut Response<'r>,
    ) {
        response.set_header(Header::new(
            "Access-Control-Allow-Origin",
            "*",
        ));
        response.set_header(Header::new(
            "Access-Control-Allow-Methods",
            "GET, POST, OPTIONS",
        ));
        response.set_header(Header::new(
            "Access-Control-Allow-Headers",
            "Content-Type",
        ));
        // Required for SharedArrayBuffer (WASM threading).
        response.set_header(Header::new(
            "Cross-Origin-Embedder-Policy",
            "require-corp",
        ));
        response.set_header(Header::new(
            "Cross-Origin-Opener-Policy",
            "same-origin",
        ));
    }
}
