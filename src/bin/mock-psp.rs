use axum::{extract::Json, http::StatusCode, routing::post, Router};
use serde::{Deserialize, Serialize};
#[derive(Deserialize)]
struct Charge {
    amount: i64,
    token: String,
    currency: String,
}
#[derive(Serialize)]
struct Reply {
    id: Option<String>,
    error: Option<&'static str>,
}
async fn charge(Json(c): Json<Charge>) -> (StatusCode, Json<Reply>) {
    let _ = c.currency;
    if c.amount <= 0 {
        return (
            StatusCode::BAD_REQUEST,
            Json(Reply {
                id: None,
                error: Some("declined"),
            }),
        );
    }
    match c.token.as_str() {
        "tok_success" | "" => (
            StatusCode::OK,
            Json(Reply {
                id: Some("mock_psp_charge".into()),
                error: None,
            }),
        ),
        "tok_insufficient_funds" => (
            StatusCode::OK,
            Json(Reply {
                id: None,
                error: Some("insufficient_funds"),
            }),
        ),
        "tok_card_declined" => (
            StatusCode::OK,
            Json(Reply {
                id: None,
                error: Some("declined"),
            }),
        ),
        "tok_timeout" => {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            (
                StatusCode::OK,
                Json(Reply {
                    id: None,
                    error: Some("timeout"),
                }),
            )
        }
        "tok_network_error" => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(Reply {
                id: None,
                error: Some("network_error"),
            }),
        ),
        _ => (
            StatusCode::OK,
            Json(Reply {
                id: None,
                error: Some("declined"),
            }),
        ),
    }
}
#[tokio::main]
async fn main() {
    let port = std::env::var("PORT").unwrap_or_else(|_| "4000".into());
    let app = Router::new()
        .route("/health", axum::routing::get(|| async { "ok" }))
        .route("/charges", post(charge));
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();
}
