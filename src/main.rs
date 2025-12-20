use axum::{Extension, Router, http::StatusCode, routing::get};
use sqlx::postgres::PgPoolOptions;
mod handler;
mod models;
mod rate_limit;

#[tokio::main]
async fn main() {
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set in .env file or environment variables");

    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await
        .expect("Failed to connect to database");

    let state = models::AppState { pool };

    let rate_limiter = rate_limit::RateLimiter::new(10, 60);

    let cleanup_limiter = rate_limiter.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(300));
        loop {
            interval.tick().await;
            cleanup_limiter.cleanup().await;
        }
    });

    let app = Router::new()
        .route("/v1/health", get(|| async { (StatusCode::OK, "OK") }))
        .route("/v1/export/{token}", get(handler::export))
        .layer(axum::middleware::from_fn(rate_limit::rate_limit_middleware))
        .layer(Extension(rate_limiter))
        .with_state(state);

    let port: u16 = std::env::var("PORT")
        .unwrap_or_else(|_| "7000".to_string())
        .parse()
        .expect("Failed to parse PORT");

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .unwrap();
    println!("Listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}
