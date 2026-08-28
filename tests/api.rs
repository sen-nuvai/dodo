use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    middleware,
    response::Response,
};
use dodo::{
    app,
    auth::bearer,
    models::{Customer, Invoice, LineItem},
    payment::PaymentStore,
    psp::MockPsp,
    AppState,
};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio::time::timeout;
use tower::ServiceExt;
use uuid::Uuid;

fn state() -> AppState {
    AppState {
        payments: Arc::new(PaymentStore::default()),
        psp: Arc::new(MockPsp::default()),
        repository: None,
    }
}

async fn body_json(response: Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}

fn seeded_invoice(s: &AppState) -> Uuid {
    let customer_id = Uuid::new_v4();
    s.payments.insert_customer(Customer {
        id: customer_id,
        email: "test@example.com".into(),
        name: "Test".into(),
    });
    let invoice_id = Uuid::new_v4();
    s.payments.insert_invoice(Invoice {
        id: invoice_id,
        customer_id,
        amount: 100,
        currency: "USD".into(),
        status: "open".into(),
        payment_id: None,
        due_date: None,
        line_items: vec![LineItem {
            quantity: 1,
            unit_amount_cents: 100,
            description: None,
        }],
    });
    invoice_id
}

async fn pay_request(router: axum::Router, id: Uuid, token: &str, key: Option<&str>) -> Response {
    let mut request =
        Request::post(format!("/invoices/{id}/pay")).header("content-type", "application/json");
    if let Some(key) = key {
        request = request.header("idempotency-key", key);
    }
    router
        .oneshot(
            request
                .body(Body::from(
                    json!({"payment_method_token": token}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn concurrent_invoice_payments_have_one_success_and_one_attempt() {
    let s = state();
    let id = seeded_invoice(&s);
    let router = app(s.clone());
    let mut jobs = Vec::new();
    for _ in 0..8 {
        jobs.push(tokio::spawn(pay_request(
            router.clone(),
            id,
            "tok_success",
            None,
        )));
    }
    let mut responses = Vec::new();
    for job in jobs {
        let response = job.await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        responses.push(body_json(response).await);
    }
    assert!(responses.iter().all(|v| v["status"] == "paid"));
    assert!(responses
        .iter()
        .all(|v| v["payment_id"] == responses[0]["payment_id"]));
    let invoice = s.payments.invoice(id).unwrap();
    assert_eq!(invoice.status, "paid");
    let payment_id = invoice.payment_id.unwrap();
    assert_eq!(s.payments.attempts(payment_id).len(), 1);
}

#[tokio::test]
async fn invoice_retry_with_same_idempotency_key_replays_response_without_psp_duplicate() {
    let s = state();
    let id = seeded_invoice(&s);
    let router = app(s.clone());
    let first = pay_request(router.clone(), id, "tok_success", Some("retry-key")).await;
    let first_body = body_json(first).await;
    let second = pay_request(router, id, "tok_success", Some("retry-key")).await;
    assert_eq!(body_json(second).await, first_body);
    let payment_id = s.payments.invoice(id).unwrap().payment_id.unwrap();
    assert_eq!(s.payments.attempts(payment_id).len(), 1);
}

#[tokio::test]
async fn timeout_and_network_error_are_bounded_and_leave_valid_attempt() {
    for token in ["tok_timeout", "tok_network_error"] {
        let s = state();
        let id = seeded_invoice(&s);
        let response = timeout(
            Duration::from_secs(3),
            pay_request(app(s.clone()), id, token, None),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let invoice = s.payments.invoice(id).unwrap();
        assert_eq!(invoice.status, "open");
        let payment_id = invoice.payment_id.unwrap();
        assert_eq!(s.payments.attempts(payment_id).len(), 1);
    }
}

#[tokio::test]
async fn health_is_available() {
    let response = app(state())
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(response.into_body(), usize::MAX).await.unwrap(),
        "ok"
    );
}

#[tokio::test]
async fn creates_payment_and_can_fetch_it() {
    let router = app(state());
    let response = router
        .clone()
        .oneshot(
            Request::post("/payments")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"amount": 1250, "currency": "usd"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let payment = body_json(response).await;
    assert_eq!(payment["amount"], 1250);
    assert_eq!(payment["currency"], "USD");
    assert_eq!(payment["status"], "succeeded");
    let id = payment["id"].as_str().unwrap();

    let fetched = router
        .oneshot(
            Request::get(format!("/payments/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(fetched.status(), StatusCode::OK);
    assert_eq!(body_json(fetched).await["id"], id);
}

#[tokio::test]
async fn rejects_invalid_amount_and_currency() {
    for input in [
        json!({"amount": 0, "currency": "USD"}),
        json!({"amount": -1, "currency": "USD"}),
        json!({"amount": 1, "currency": "US"}),
    ] {
        let response = app(state())
            .oneshot(
                Request::post("/payments")
                    .header("content-type", "application/json")
                    .body(Body::from(input.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}

#[tokio::test]
async fn unknown_payment_is_not_found() {
    let response = app(state())
        .oneshot(
            Request::get("/payments/00000000-0000-0000-0000-000000000000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn bearer_middleware_rejects_missing_or_wrong_token() {
    std::env::set_var("API_TOKEN", "test-secret");
    let protected = app(state()).layer(middleware::from_fn(bearer));
    for authorization in [None, Some("Bearer wrong")] {
        let mut request = Request::get("/health");
        if let Some(value) = authorization {
            request = request.header("authorization", value);
        }
        let response = protected
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    let response = protected
        .oneshot(
            Request::get("/health")
                .header("authorization", "Bearer test-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    std::env::remove_var("API_TOKEN");
}
