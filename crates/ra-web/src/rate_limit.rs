//! Simple in-memory rate limiting fairing.
//!
//! Limits requests per IP address using a sliding window.
//! Requests exceeding the limit receive a 429 Too Many Requests
//! response.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rocket::fairing::{Fairing, Info, Kind};
use rocket::http::Status;
use rocket::{Data, Request};

/// Per-IP request tracking.
struct IpBucket {
    timestamps: Vec<Instant>,
}

impl IpBucket {
    fn new() -> Self {
        Self {
            timestamps: Vec::new(),
        }
    }

    /// Remove timestamps older than `window` and return the
    /// current request count within the window.
    fn count_within(&mut self, window: Duration) -> usize {
        let now = Instant::now();
        let cutoff = now.checked_sub(window).unwrap_or(now);
        self.timestamps.retain(|t| *t > cutoff);
        self.timestamps.len()
    }

    fn record(&mut self) {
        self.timestamps.push(Instant::now());
    }
}

/// Rate limiter configuration and state.
pub struct RateLimiter {
    max_requests: usize,
    window: Duration,
    buckets: Mutex<HashMap<IpAddr, IpBucket>>,
}

impl RateLimiter {
    /// Create a rate limiter allowing `max_requests` per `window`.
    #[must_use]
    pub fn new(max_requests: usize, window: Duration) -> Self {
        Self {
            max_requests,
            window,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// Check whether the given IP is allowed and record the
    /// request. Returns `true` if allowed.
    fn check_and_record(&self, ip: IpAddr) -> bool {
        let mut buckets = self
            .buckets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let bucket =
            buckets.entry(ip).or_insert_with(IpBucket::new);
        let count = bucket.count_within(self.window);

        if count >= self.max_requests {
            return false;
        }

        bucket.record();
        true
    }
}

#[rocket::async_trait]
impl Fairing for RateLimiter {
    fn info(&self) -> Info {
        Info {
            name: "Rate Limiter",
            kind: Kind::Request,
        }
    }

    async fn on_request(
        &self,
        request: &mut Request<'_>,
        _data: &mut Data<'_>,
    ) {
        // Skip rate limiting for health checks and static assets.
        let path = request.uri().path().as_str();
        if path == "/health" || path == "/" {
            return;
        }

        let Some(ip) = request.client_ip() else {
            return;
        };

        if !self.check_and_record(ip) {
            request.local_cache(|| RateLimited(true));
        }
    }
}

/// Marker cached in the request when rate limited.
#[derive(Default)]
pub struct RateLimited(pub bool);

/// Request guard that rejects rate-limited requests.
///
/// Add this as a parameter to any route handler to enforce
/// rate limiting on that endpoint.
pub struct RateGuard;

#[rocket::async_trait]
impl<'r> rocket::request::FromRequest<'r> for RateGuard {
    type Error = ();

    async fn from_request(
        request: &'r Request<'_>,
    ) -> rocket::request::Outcome<Self, Self::Error> {
        let limited = request
            .local_cache(|| RateLimited(false));
        if limited.0 {
            rocket::request::Outcome::Error((
                Status::TooManyRequests,
                (),
            ))
        } else {
            rocket::request::Outcome::Success(RateGuard)
        }
    }
}
