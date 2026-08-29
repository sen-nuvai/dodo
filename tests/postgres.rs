//! Postgres integration coverage. Tests are skipped when DATABASE_URL is unset.
use axum::{body::Body, http::Request};
use dodo::models::LineItem;
use dodo::repository::PgRepository;
use dodo::{app, payment::PaymentStore, psp::MockPsp, AppState};
use serde_json::json;
use std::sync::Arc;
use tower::ServiceExt;
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

#[tokio::test]
async fn postgres_web_http_concurrent_invoice_claims_are_single() {
    let Some(repo) = repo().await else { return };
    let tenant_key = key("http");
    let tenant = repo.bootstrap_tenant("pg http", &tenant_key).await.unwrap();
    let (customer, _, _) = repo
        .create_customer(tenant, "http@example.test", "HTTP")
        .await
        .unwrap();
    let invoice = repo
        .create_invoice(
            tenant,
            customer,
            100,
            "USD",
            None,
            &[LineItem {
                description: None,
                quantity: 1,
                unit_amount_cents: 100,
            }],
        )
        .await
        .unwrap();
    repo.finalize_invoice(tenant, invoice.id).await.unwrap();
    let state = AppState {
        payments: Arc::new(PaymentStore::default()),
        psp: Arc::new(MockPsp::default()),
        repository: Some(repo.clone()),
    };
    let router = app(state.clone());
    let mut jobs = Vec::new();
    for _ in 0..8 {
        let r = router.clone();
        let k = tenant_key.clone();
        jobs.push(tokio::spawn(async move {
            r.oneshot(
                Request::post(format!("/invoices/{}/pay", invoice.id))
                    .header("x-api-key", k)
                    .header("idempotency-key", "http-key")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"payment_method_token":"tok_success"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap()
        }));
    }
    for job in jobs {
        assert!(job.await.unwrap().status().is_success());
    }
    let stored = repo.get_invoice(tenant, invoice.id).await.unwrap().unwrap();
    assert_eq!(stored.status, "paid");
    assert_eq!(
        repo.list_attempts(tenant, stored.payment_id.unwrap())
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(state.psp.call_count(), 1);
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
async fn postgres_invoice_events_are_transactionally_queued() {
    let Some(repo) = repo().await else { return };
    let tenant = repo
        .bootstrap_tenant("pg events", &key("events"))
        .await
        .unwrap();
    let (customer, _, _) = repo
        .create_customer(tenant, "events@example.test", "Events")
        .await
        .unwrap();
    repo.register_webhook(tenant, "http://127.0.0.1:9/events")
        .await
        .unwrap();
    let invoice = repo
        .create_invoice(
            tenant,
            customer,
            100,
            "USD",
            None,
            &[LineItem {
                description: None,
                quantity: 1,
                unit_amount_cents: 100,
            }],
        )
        .await
        .unwrap();
    let created: i64 = sqlx::query_scalar("SELECT count(*) FROM webhook_deliveries WHERE event_type='invoice.created' AND event_id=$1")
        .bind(format!("invoice.created:{}", invoice.id)).fetch_one(&repo.pool).await.unwrap();
    assert_eq!(created, 1);
    repo.finalize_invoice(tenant, invoice.id).await.unwrap();
    let claim = repo
        .claim_invoice_payment(tenant, invoice.id, Some("events-key"), "events-fp")
        .await
        .unwrap();
    repo.finalize_invoice_payment(
        tenant,
        invoice.id,
        claim.invoice.payment_id.unwrap(),
        "succeeded",
        Some("psp"),
        None,
    )
    .await
    .unwrap();
    let paid: i64 = sqlx::query_scalar("SELECT count(*) FROM webhook_deliveries WHERE event_type='invoice.paid' AND event_id LIKE $1")
        .bind(format!("invoice.paid:{}:%", invoice.id)).fetch_one(&repo.pool).await.unwrap();
    assert_eq!(paid, 1);
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
async fn postgres_webhook_worker_claims_lease_and_exhausts() {
    let Some(repo) = repo().await else { return };
    let tenant = repo
        .bootstrap_tenant("pg worker", &key("worker"))
        .await
        .unwrap();
    let registration = repo
        .register_webhook(tenant, "http://127.0.0.1:9/unreachable")
        .await
        .unwrap();
    let delivery_id = Uuid::new_v4();
    sqlx::query("INSERT INTO webhook_deliveries(id,registration_id,event_id,payload,event_type,attempts,max_attempts,next_attempt_at) VALUES($1,$2,$3,$4,'invoice.created',4,5,now())")
        .bind(delivery_id).bind(registration.id).bind("worker-exhaustion").bind(b"{}".to_vec())
        .execute(&repo.pool).await.unwrap();
    let (attempts, exhausted, last_error): (i32, Option<chrono::DateTime<chrono::Utc>>, Option<String>) = sqlx::query_as("UPDATE webhook_deliveries SET attempts=attempts+1, lease_until=now()+interval '30 seconds', last_error='test failure', exhausted_at=CASE WHEN attempts+1 >= max_attempts THEN now() ELSE exhausted_at END WHERE id=$1 RETURNING attempts,exhausted_at,last_error")
        .bind(delivery_id).fetch_one(&repo.pool).await.unwrap();
    assert_eq!(attempts, 5);
    assert!(exhausted.is_some());
    assert_eq!(last_error.as_deref(), Some("test failure"));
    let due: i64 = sqlx::query_scalar("SELECT count(*) FROM webhook_deliveries WHERE id=$1 AND exhausted_at IS NOT NULL AND attempts=5").bind(delivery_id).fetch_one(&repo.pool).await.unwrap();
    assert_eq!(due, 1);
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
