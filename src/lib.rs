pub mod api;
pub mod auth;
pub mod error;
pub mod models;
pub mod payment;
pub mod psp;
pub mod repository;
use axum::{
    routing::{get, post},
    Router,
};
use payment::PaymentStore;
use psp::MockPsp;
use std::sync::Arc;
#[derive(Clone)]
pub struct AppState {
    pub payments: Arc<PaymentStore>,
    pub psp: Arc<MockPsp>,
    pub repository: Option<Arc<repository::PgRepository>>,
}
pub fn app(state: AppState) -> Router {
    Router::new()
        .layer(axum::middleware::from_fn(auth::bearer))
        .route("/health", get(api::health))
        .route("/payments", post(api::create_payment))
        .route("/payments/:id", get(api::get_payment))
        .route(
            "/customers",
            get(api::list_customers).post(api::create_customer),
        )
        .route(
            "/customers/:id",
            get(api::get_customer).put(api::update_customer),
        )
        .route(
            "/invoices",
            get(api::list_invoices).post(api::create_invoice),
        )
        .route("/invoices/:id", get(api::get_invoice))
        .route("/invoices/:id/finalize", post(api::finalize_invoice))
        .route("/invoices/:id/pay", post(api::pay_invoice))
        .route("/payments/:id/attempts", get(api::get_attempts))
        .route("/webhooks", post(api::register_webhook))
        .route("/webhooks/mock", post(api::webhook))
        .route("/webhooks/:registration_id", post(api::webhook_scoped))
        .with_state(state)
}
