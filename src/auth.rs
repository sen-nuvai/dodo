use crate::error::AppError;
use axum::{extract::Request, middleware::Next, response::Response};

/// Bearer/API-key auth. API_TOKEN remains supported for local development; production
/// callers should provide X-API-Key and configure tenant keys in Postgres.
pub async fn bearer(request: Request, next: Next) -> Result<Response, AppError> {
    if let Ok(expected) = std::env::var("API_TOKEN") {
        let auth = request
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok());
        let api_key = request
            .headers()
            .get("x-api-key")
            .and_then(|v| v.to_str().ok());
        let valid =
            auth == Some(&format!("Bearer {expected}")) || api_key == Some(expected.as_str());
        if !valid {
            return Err(AppError::Unauthorized);
        }
    }
    Ok(next.run(request).await)
}
