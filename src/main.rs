use axum::{Extension, Router, http::StatusCode, routing::get};
use s3::{Bucket, Region, creds::Credentials};
use sqlx::postgres::PgPoolOptions;
mod handler;
mod models;
mod rate_limit;
mod s3_helpers;

#[tokio::main]
async fn main() {
    #[cfg(debug_assertions)]
    dotenvy::dotenv().ok();

    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set in .env file or environment variables");

    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await
        .expect("Failed to connect to database");

    let bucket_name = std::env::var("S3_BUCKET")
        .expect("S3_BUCKET must be set in .env file or environment variables");

    let access_key = std::env::var("S3_ACCESS_KEY_ID")
        .expect("S3_ACCESS_KEY_ID must be set in .env file or environment variables");

    let secret_key = std::env::var("S3_SECRET_ACCESS_KEY")
        .expect("S3_SECRET_ACCESS_KEY must be set in .env file or environment variables");

    let credentials = Credentials::new(Some(&access_key), Some(&secret_key), None, None, None)
        .expect("Failed to create S3 credentials");

    let region = if let Ok(endpoint) = std::env::var("S3_ENDPOINT") {
        let region_name = std::env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".to_string());
        Region::Custom {
            region: region_name,
            endpoint,
        }
    } else {
        std::env::var("S3_REGION")
            .unwrap_or_else(|_| "us-east-1".to_string())
            .parse::<Region>()
            .expect("Invalid S3_REGION")
    };

    let s3_bucket =
        *Bucket::new(&bucket_name, region, credentials).expect("Failed to create S3 bucket");

    let state = models::AppState { pool, s3_bucket };

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
