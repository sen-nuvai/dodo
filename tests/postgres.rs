//! Postgres integration coverage. Tests are skipped when DATABASE_URL is unset.
use dodo::models::LineItem;
use dodo::repository::PgRepository;
use std::sync::Arc;
use uuid::Uuid;

async fn repo() -> Option<Arc<PgRepository>> {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        return None;
    };
    let repo = PgRepository::connect(&url)
        .await
        .expect("DATABASE_URL was set but PostgreSQL connection failed");
    repo.migrate()
        .await
        .expect("DATABASE_URL was set but migrations failed");
    Some(Arc::new(repo))
}

fn key(label: &str) -> String {
    format!("pgtest{label}{}_secret", Uuid::new_v4().simple())
}

#[tokio::test]
async fn postgres_concurrent_idempotency_returns_one_payment() {
    let Some(repo) = repo().await else { return };
    let tenant = repo
        .bootstrap_tenant("pg concurrency", &key("concurrency"))
        .await
        .unwrap();
    let repo_a = repo.clone();
    let repo_b = repo.clone();
    let k = "same-key";
    let a = tokio::spawn(async move {
        repo_a
            .create_payment(tenant, 100, "USD", Some(k), "fp", None, "succeeded")
            .await
    });
    let b = tokio::spawn(async move {
        repo_b
            .create_payment(tenant, 100, "USD", Some(k), "fp", None, "succeeded")
            .await
    });
    let (a, b) = tokio::join!(a, b);
    let a = a.unwrap().unwrap();
    let b = b.unwrap().unwrap();
    assert_eq!(a.id, b.id);
    assert_eq!(repo.list_attempts(tenant, a.id).await.unwrap().len(), 1);
}

#[tokio::test]
async fn postgres_idempotency_conflict_and_timeout_are_persisted() {
    let Some(repo) = repo().await else { return };
    let tenant = repo
        .bootstrap_tenant("pg timeout", &key("timeout"))
        .await
        .unwrap();
    let first = repo
        .create_payment(
            tenant,
            101,
            "USD",
            Some("timeout-key"),
            "one",
            None,
            "pending",
        )
        .await
        .unwrap();
    assert_eq!(first.status, "pending");
    assert!(matches!(
        repo.create_payment(
            tenant,
            102,
            "USD",
            Some("timeout-key"),
            "two",
            None,
            "succeeded"
        )
        .await,
        Err(dodo::repository::RepositoryError::IdempotencyConflict)
    ));
}

#[tokio::test]
async fn postgres_tenant_scoping_and_invoice_atomic_payment() {
    let Some(repo) = repo().await else { return };
    let tenant_a = repo
        .bootstrap_tenant("pg tenant a", &key("tenant-a"))
        .await
        .unwrap();
    let tenant_b = repo
        .bootstrap_tenant("pg tenant b", &key("tenant-b"))
        .await
        .unwrap();
    let (customer, _, _) = repo
        .create_customer(tenant_a, "a@example.test", "A")
        .await
        .unwrap();
    assert!(repo
        .create_invoice(
            tenant_b,
            customer,
            100,
            "USD",
            None,
            &[LineItem {
                description: Some("x".to_owned()),
                quantity: 1,
                unit_amount_cents: 100
            }]
        )
        .await
        .is_err());
    let invoice = repo
        .create_invoice(
            tenant_a,
            customer,
            100,
            "USD",
            None,
            &[LineItem {
                description: Some("x".to_owned()),
                quantity: 1,
                unit_amount_cents: 100,
            }],
        )
        .await
        .unwrap();
    let paid = repo
        .pay_invoice(
            tenant_a,
            invoice.id,
            Some("invoice-key"),
            "invoice-fp",
            "succeeded",
            Some("psp-1"),
            None,
        )
        .await
        .unwrap();
    assert_eq!(paid.status, "paid");
    assert_eq!(
        repo.list_attempts(tenant_a, paid.payment_id.unwrap())
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn postgres_webhook_signature_replay_and_tenant_isolation() {
    let Some(repo) = repo().await else { return };
    let tenant_a = repo
        .bootstrap_tenant("pg webhook a", &key("webhook-a"))
        .await
        .unwrap();
    let tenant_b = repo
        .bootstrap_tenant("pg webhook b", &key("webhook-b"))
        .await
        .unwrap();
    let registration = repo
        .register_webhook(tenant_a, "http://127.0.0.1:9/hook")
        .await
        .unwrap();
    let payment = repo
        .create_payment(
            tenant_a,
            100,
            "USD",
            None,
            "webhook-payment",
            None,
            "pending",
        )
        .await
        .unwrap();
    let body = serde_json::to_vec(
        &serde_json::json!({"event_id":"evt-pg", "payment_id":payment.id, "status":"succeeded"}),
    )
    .unwrap();
    let timestamp = dodo::models::webhook_timestamp();
    let signature = dodo::models::webhook_signature(&registration.secret, timestamp, &body);
    assert!(repo
        .apply_webhook(
            tenant_a,
            "evt-pg",
            payment.id,
            "succeeded",
            Some("psp"),
            &body,
            timestamp,
            &signature,
            None
        )
        .await
        .unwrap());
    assert!(repo
        .apply_webhook(
            tenant_a,
            "evt-pg",
            payment.id,
            "succeeded",
            Some("psp"),
            &body,
            timestamp,
            &signature,
            None
        )
        .await
        .unwrap());
    assert!(repo
        .apply_webhook(
            tenant_b,
            "evt-pg-b",
            payment.id,
            "succeeded",
            None,
            &body,
            timestamp,
            &signature,
            None
        )
        .await
        .is_err());
}
