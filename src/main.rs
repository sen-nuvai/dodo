use dodo::{app, payment::PaymentStore, psp::MockPsp, repository::PgRepository, AppState};
use std::sync::Arc;
#[tokio::main]
async fn main() {
    let repository = if let Ok(url) = std::env::var("DATABASE_URL") {
        let repo = PgRepository::connect(&url)
            .await
            .expect("database connection");
        repo.migrate().await.expect("database migration");
        let repo = Arc::new(repo);
        if let Ok(key) = std::env::var("BOOTSTRAP_API_KEY") {
            repo.bootstrap_tenant("default", &key)
                .await
                .expect("bootstrap tenant");
        }
        tokio::spawn(dodo::repository::run_delivery_worker((*repo).clone()));
        Some(repo)
    } else {
        None
    };
    let state = AppState {
        payments: Arc::new(PaymentStore::default()),
        psp: Arc::new(MockPsp::default()),
        repository,
    };
    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".into());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .expect("bind");
    println!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app(state)).await.expect("server");
}
