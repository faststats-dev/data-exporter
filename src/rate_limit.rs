use axum::{extract::Request, http::StatusCode, middleware::Next, response::Response};
use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct RateLimiter {
    requests: Arc<RwLock<HashMap<String, Vec<DateTime<Utc>>>>>,
    max_requests: usize,
    window_seconds: i64,
}

impl RateLimiter {
    pub fn new(max_requests: usize, window_seconds: i64) -> Self {
        Self {
            requests: Arc::new(RwLock::new(HashMap::new())),
            max_requests,
            window_seconds,
        }
    }

    pub async fn check_rate_limit(&self, key: &str) -> bool {
        let mut requests = self.requests.write().await;
        let now = Utc::now();
        let window_start = now - Duration::seconds(self.window_seconds);

        let entry = requests.entry(key.to_string()).or_insert_with(Vec::new);

        entry.retain(|&timestamp| timestamp > window_start);

        if entry.len() >= self.max_requests {
            return false;
        }

        entry.push(now);
        true
    }

    pub async fn cleanup(&self) {
        let mut requests = self.requests.write().await;
        let now = Utc::now();
        let window_start = now - Duration::seconds(self.window_seconds);

        requests.retain(|_, timestamps| {
            timestamps.retain(|&timestamp| timestamp > window_start);
            !timestamps.is_empty()
        });
    }
}

pub async fn rate_limit_middleware(request: Request, next: Next) -> Result<Response, StatusCode> {
    let limiter = request
        .extensions()
        .get::<RateLimiter>()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?
        .clone();

    let ip = request
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");

    if !limiter.check_rate_limit(ip).await {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }

    Ok(next.run(request).await)
}
