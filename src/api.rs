use crate::{error::AppError, models::*, psp::PspError, AppState};
use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::time::Duration;
use uuid::Uuid;
pub async fn health() -> &'static str {
    "ok"
}
async fn tenant(s: &AppState, h: &HeaderMap) -> Result<Option<Uuid>, AppError> {
    let Some(repo) = &s.repository else {
        return Ok(None);
    };
    let raw = h
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .ok_or(AppError::Unauthorized)?;
    repo.authenticate_api_key(raw)
        .await
        .map_err(|_| AppError::Internal)?
        .ok_or(AppError::Unauthorized)
        .map(Some)
}
fn key(h: &HeaderMap) -> Option<String> {
    h.get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}
pub async fn create_payment(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<CreatePayment>,
) -> Result<(StatusCode, Json<Payment>), AppError> {
    let c = i.currency.trim().to_uppercase();
    if c.len() != 3 || !c.bytes().all(|b| b.is_ascii_alphabetic()) {
        return Err(AppError::BadRequest(
            "currency must be ISO-4217 code".into(),
        ));
    }
    let amount = i.total().map_err(AppError::BadRequest)?;
    let k = key(&h);
    if k.as_deref().is_some_and(str::is_empty) {
        return Err(AppError::BadRequest(
            "idempotency-key cannot be empty".into(),
        ));
    }
    let fp = canonical_fingerprint(&i);
    if let Some(repo) = &s.repository {
        let raw = h
            .get("x-api-key")
            .and_then(|v| v.to_str().ok())
            .ok_or(AppError::Unauthorized)?;
        let tenant = repo
            .authenticate_api_key(raw)
            .await
            .map_err(|_| AppError::Internal)?
            .ok_or(AppError::Unauthorized)?;
        let status = match i.payment_method_token.as_deref() {
            Some("tok_timeout") => "pending",
            Some("tok_card_declined")
            | Some("tok_insufficient_funds")
            | Some("tok_network_error") => "failed",
            _ => "succeeded",
        };
        let stored = repo
            .create_payment(tenant, amount, &c, k.as_deref(), &fp, None, status)
            .await
            .map_err(|e| match e {
                crate::repository::RepositoryError::IdempotencyConflict => AppError::Conflict,
                _ => AppError::Internal,
            })?;
        let status_enum = match stored.status.as_str() {
            "succeeded" => PaymentStatus::Succeeded,
            "failed" => PaymentStatus::Failed,
            _ => PaymentStatus::Pending,
        };
        return Ok((
            StatusCode::CREATED,
            Json(Payment {
                id: stored.id,
                amount: stored.amount,
                currency: stored.currency,
                status: status_enum,
                psp_id: stored.provider_id,
                idempotency_key: k,
                attempts: stored.attempts as u32,
            }),
        ));
    }
    let p = s
        .payments
        .create(&s.psp, amount, c, k, fp, i.payment_method_token.as_deref())
        .await
        .map_err(|conf| {
            if conf {
                AppError::Conflict
            } else {
                AppError::Internal
            }
        })?;
    Ok((
        if p.status == PaymentStatus::Succeeded {
            StatusCode::CREATED
        } else {
            StatusCode::PAYMENT_REQUIRED
        },
        Json(p),
    ))
}
pub async fn get_payment(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Payment>, AppError> {
    if let Some(repo) = &s.repository {
        let t = tenant(&s, &h).await?.unwrap();
        let p = repo
            .get_payment(t, id)
            .await
            .map_err(|_| AppError::Internal)?
            .ok_or(AppError::NotFound)?;
        let status = match p.status.as_str() {
            "succeeded" => PaymentStatus::Succeeded,
            "failed" => PaymentStatus::Failed,
            _ => PaymentStatus::Pending,
        };
        return Ok(Json(Payment {
            id: p.id,
            amount: p.amount,
            currency: p.currency,
            status,
            psp_id: p.provider_id,
            idempotency_key: None,
            attempts: p.attempts as u32,
        }));
    }
    s.payments.get(id).map(Json).ok_or(AppError::NotFound)
}
pub async fn create_customer(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<CreateCustomer>,
) -> Result<(StatusCode, Json<Customer>), AppError> {
    if i.email.trim().is_empty() {
        return Err(AppError::BadRequest("email required".into()));
    }
    if let Some(repo) = &s.repository {
        let t = tenant(&s, &h).await?.unwrap();
        let (id, email, name) = repo
            .create_customer(t, &i.email, &i.name)
            .await
            .map_err(|_| AppError::Internal)?;
        return Ok((StatusCode::CREATED, Json(Customer { id, email, name })));
    }
    let c = Customer {
        id: Uuid::new_v4(),
        email: i.email,
        name: i.name,
    };
    s.payments.insert_customer(c.clone());
    Ok((StatusCode::CREATED, Json(c)))
}
pub async fn list_customers(
    State(s): State<AppState>,
    h: HeaderMap,
) -> Result<Json<Vec<Customer>>, AppError> {
    if let Some(repo) = &s.repository {
        let t = tenant(&s, &h).await?.unwrap();
        let rows = repo
            .list_customers(t)
            .await
            .map_err(|_| AppError::Internal)?;
        return Ok(Json(
            rows.into_iter()
                .map(|(id, email, name)| Customer { id, email, name })
                .collect(),
        ));
    }
    Ok(Json(
        s.payments
            .customers
            .read()
            .map(|m| m.values().cloned().collect())
            .unwrap_or_default(),
    ))
}
pub async fn get_customer(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Customer>, AppError> {
    if let Some(repo) = &s.repository {
        let t = tenant(&s, &h).await?.unwrap();
        let (id, email, name) = repo
            .get_customer(t, id)
            .await
            .map_err(|_| AppError::Internal)?
            .ok_or(AppError::NotFound)?;
        return Ok(Json(Customer { id, email, name }));
    }
    s.payments.customer(id).map(Json).ok_or(AppError::NotFound)
}
pub async fn update_customer(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<UpdateCustomer>,
) -> Result<Json<Customer>, AppError> {
    if let Some(repo) = &s.repository {
        let t = tenant(&s, &h).await?.unwrap();
        let (id, email, name) = repo
            .update_customer(t, id, i.email.as_deref(), i.name.as_deref())
            .await
            .map_err(|_| AppError::Internal)?
            .ok_or(AppError::NotFound)?;
        return Ok(Json(Customer { id, email, name }));
    }
    s.payments
        .update_customer(id, i.email, i.name)
        .map(Json)
        .ok_or(AppError::NotFound)
}
pub async fn get_invoice(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Invoice>, AppError> {
    if let Some(repo) = &s.repository {
        let i = repo
            .get_invoice(tenant(&s, &h).await?.unwrap(), id)
            .await
            .map_err(|_| AppError::Internal)?
            .ok_or(AppError::NotFound)?;
        return Ok(Json(Invoice {
            id: i.id,
            customer_id: i.customer_id,
            amount: i.amount,
            currency: i.currency,
            status: i.status,
            payment_id: i.payment_id,
            due_date: i.due_date,
            line_items: i.line_items,
        }));
    }
    s.payments.invoice(id).map(Json).ok_or(AppError::NotFound)
}
pub async fn list_invoices(
    State(s): State<AppState>,
    h: HeaderMap,
    Query(q): Query<InvoiceFilter>,
) -> Result<Json<Vec<Invoice>>, AppError> {
    if let Some(repo) = &s.repository {
        let rows = repo
            .list_invoices(
                tenant(&s, &h).await?.unwrap(),
                q.status.as_deref(),
                q.customer_id,
            )
            .await
            .map_err(|_| AppError::Internal)?;
        return Ok(Json(
            rows.into_iter()
                .map(|i| Invoice {
                    id: i.id,
                    customer_id: i.customer_id,
                    amount: i.amount,
                    currency: i.currency,
                    status: i.status,
                    payment_id: i.payment_id,
                    due_date: i.due_date,
                    line_items: i.line_items,
                })
                .collect(),
        ));
    }
    Ok(Json(
        s.payments.invoices(q.status.as_deref(), q.customer_id),
    ))
}
pub async fn create_invoice(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<CreateInvoice>,
) -> Result<(StatusCode, Json<Invoice>), AppError> {
    let repo_tenant = if s.repository.is_some() {
        Some(tenant(&s, &h).await?.unwrap())
    } else {
        None
    };
    if s.repository.is_none() && s.payments.customer(i.customer_id).is_none() {
        return Err(AppError::NotFound);
    }
    let c = i.currency.trim().to_uppercase();
    if c.len() != 3 {
        return Err(AppError::BadRequest("invalid currency".into()));
    }
    let amount = (CreatePayment {
        amount: None,
        currency: c.clone(),
        line_items: Some(i.line_items.clone()),
        payment_method_token: None,
    })
    .total()
    .map_err(AppError::BadRequest)?;
    let inv = Invoice {
        id: Uuid::new_v4(),
        customer_id: i.customer_id,
        amount,
        currency: c,
        status: "draft".into(),
        payment_id: None,
        due_date: i.due_date,
        line_items: i.line_items.clone(),
    };
    if let (Some(repo), Some(t)) = (&s.repository, repo_tenant) {
        let x = repo
            .create_invoice(
                t,
                i.customer_id,
                amount,
                &inv.currency,
                inv.due_date,
                &inv.line_items,
            )
            .await
            .map_err(|e| match e {
                crate::repository::RepositoryError::NotFound => AppError::NotFound,
                _ => AppError::Internal,
            })?;
        return Ok((
            StatusCode::CREATED,
            Json(Invoice {
                id: x.id,
                customer_id: x.customer_id,
                amount: x.amount,
                currency: x.currency,
                status: x.status,
                payment_id: x.payment_id,
                due_date: x.due_date,
                line_items: x.line_items,
            }),
        ));
    }
    s.payments.insert_invoice(inv.clone());
    Ok((StatusCode::CREATED, Json(inv)))
}
pub async fn finalize_invoice(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Invoice>, AppError> {
    let map = |i: crate::repository::StoredInvoice| Invoice {
        id: i.id,
        customer_id: i.customer_id,
        amount: i.amount,
        currency: i.currency,
        status: i.status,
        payment_id: i.payment_id,
        due_date: i.due_date,
        line_items: i.line_items,
    };
    if let Some(repo) = &s.repository {
        let i = repo
            .finalize_invoice(tenant(&s, &h).await?.unwrap(), id)
            .await
            .map_err(|e| match e {
                crate::repository::RepositoryError::NotFound => AppError::NotFound,
                _ => AppError::Internal,
            })?;
        return Ok(Json(map(i)));
    }
    let i = s
        .payments
        .finalize_invoice(id)
        .await
        .ok_or(AppError::Conflict)?;
    Ok(Json(i))
}

pub async fn pay_invoice(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(req): Json<PayInvoice>,
) -> Result<Json<Invoice>, AppError> {
    if let Some(repo) = &s.repository {
        let t = tenant(&s, &h).await?.unwrap();
        let invoice = repo
            .get_invoice(t, id)
            .await
            .map_err(|_| AppError::Internal)?
            .ok_or(AppError::NotFound)?;
        if invoice.status == "paid" {
            return Ok(Json(Invoice {
                id: invoice.id,
                customer_id: invoice.customer_id,
                amount: invoice.amount,
                currency: invoice.currency,
                status: invoice.status,
                payment_id: invoice.payment_id,
                due_date: invoice.due_date,
                line_items: invoice.line_items,
            }));
        }
        let fp = hex::encode(Sha256::digest(
            format!("{}:{}:{}", id, invoice.amount, req.payment_method_token).as_bytes(),
        ));
        let claim = repo
            .claim_invoice_payment(t, id, key(&h).as_deref(), &fp)
            .await
            .map_err(|e| match e {
                crate::repository::RepositoryError::NotFound => AppError::NotFound,
                crate::repository::RepositoryError::IdempotencyConflict => AppError::Conflict,
                _ => AppError::Internal,
            })?;
        if !claim.claimed {
            return Ok(Json(Invoice {
                id: claim.invoice.id,
                customer_id: claim.invoice.customer_id,
                amount: claim.invoice.amount,
                currency: claim.invoice.currency,
                status: claim.invoice.status,
                payment_id: claim.invoice.payment_id,
                due_date: claim.invoice.due_date,
                line_items: claim.invoice.line_items,
            }));
        }
        let invoice = claim.invoice;
        let charge = tokio::time::timeout(
            Duration::from_secs(2),
            s.psp.charge(
                invoice.amount,
                &invoice.currency,
                Some(&req.payment_method_token),
            ),
        )
        .await;
        let (status, provider, error) = match charge {
            Ok(Ok(c)) => ("succeeded", Some(c.id), None),
            Ok(Err(PspError::Timeout)) | Err(_) => ("pending", None, Some("timeout".to_owned())),
            Ok(Err(e)) => ("failed", None, Some(e.code().to_owned())),
        };
        let x = repo
            .finalize_invoice_payment(
                t,
                id,
                invoice.payment_id.unwrap(),
                status,
                provider.as_deref(),
                error.as_deref(),
            )
            .await
            .map_err(|e| match e {
                crate::repository::RepositoryError::NotFound => AppError::NotFound,
                crate::repository::RepositoryError::IdempotencyConflict => AppError::Conflict,
                _ => AppError::Internal,
            })?;
        return Ok(Json(Invoice {
            id: x.id,
            customer_id: x.customer_id,
            amount: x.amount,
            currency: x.currency,
            status: x.status,
            payment_id: x.payment_id,
            due_date: x.due_date,
            line_items: x.line_items,
        }));
    }
    let i = s
        .payments
        .pay_invoice(&s.psp, id, key(&h), &req.payment_method_token)
        .await
        .map_err(|c| {
            if c {
                AppError::Conflict
            } else {
                AppError::NotFound
            }
        })?;
    Ok(Json(i))
}
pub async fn get_attempts(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<PaymentAttempt>>, AppError> {
    if let Some(repo) = &s.repository {
        let rows = repo
            .list_attempts(tenant(&s, &h).await?.unwrap(), id)
            .await
            .map_err(|_| AppError::Internal)?;
        return Ok(Json(
            rows.into_iter()
                .map(|(id, payment_id, status, psp_id, error)| PaymentAttempt {
                    id,
                    payment_id,
                    status: match status.as_str() {
                        "succeeded" => PaymentStatus::Succeeded,
                        "failed" => PaymentStatus::Failed,
                        _ => PaymentStatus::Pending,
                    },
                    psp_id,
                    error,
                })
                .collect(),
        ));
    }
    if s.payments.get(id).is_none() {
        return Err(AppError::NotFound);
    }
    Ok(Json(s.payments.attempts(id)))
}
pub async fn register_webhook(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<WebhookRegistration>,
) -> Result<(StatusCode, Json<WebhookRegistrationResponse>), AppError> {
    if !i.url.starts_with("http://") && !i.url.starts_with("https://") {
        return Err(AppError::BadRequest("url must be http(s)".into()));
    }
    if let Some(repo) = &s.repository {
        let x = repo
            .register_webhook(tenant(&s, &h).await?.unwrap(), &i.url)
            .await
            .map_err(|_| AppError::Internal)?;
        return Ok((
            StatusCode::CREATED,
            Json(WebhookRegistrationResponse {
                id: x.id,
                url: x.url,
                secret: x.secret,
            }),
        ));
    }
    Ok((StatusCode::CREATED, Json(s.payments.register(i.url))))
}
pub async fn webhook_scoped(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(registration_id): Path<Uuid>,
    body: Bytes,
) -> Result<StatusCode, AppError> {
    webhook_inner(s, h, body, Some(registration_id)).await
}
pub async fn webhook(
    State(s): State<AppState>,
    h: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, AppError> {
    webhook_inner(s, h, body, None).await
}
async fn webhook_inner(
    s: AppState,
    h: HeaderMap,
    body: Bytes,
    registration_id: Option<Uuid>,
) -> Result<StatusCode, AppError> {
    if let Some(repo) = &s.repository {
        let t = tenant(&s, &h).await?.unwrap();
        let sig = h
            .get("x-webhook-signature")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        repo.verify_webhook_signature(t, registration_id, sig, &body)
            .await
            .map_err(|_| AppError::Unauthorized)?;
    }
    if let Some(repo) = &s.repository {
        let e: WebhookEvent = serde_json::from_slice(&body)
            .map_err(|_| AppError::BadRequest("invalid event".into()))?;
        let t = tenant(&s, &h).await?.unwrap();
        let sig = h
            .get("x-webhook-signature")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let changed = repo
            .apply_webhook(
                t,
                &e.event_id,
                e.payment_id,
                e.status.as_str(),
                e.psp_id.as_deref(),
                &body,
                sig,
                registration_id,
            )
            .await
            .map_err(|_| AppError::Unauthorized)?;
        return if changed {
            Ok(StatusCode::NO_CONTENT)
        } else {
            Err(AppError::NotFound)
        };
    }
    let e: WebhookEvent =
        serde_json::from_slice(&body).map_err(|_| AppError::BadRequest("invalid event".into()))?;
    let target = s.payments.webhooks.read().unwrap().values().next().cloned();
    if let Some(t) = target {
        let sig = h
            .get("x-webhook-signature")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let mut mac = Hmac::<Sha256>::new_from_slice(t.secret.as_bytes()).unwrap();
        mac.update(&body);
        let expected = hex::encode(mac.finalize().into_bytes());
        if sig != expected {
            return Err(AppError::Unauthorized);
        }
    }
    let event = e.clone();
    match s.payments.apply_event(
        e.event_id.clone(),
        e.payment_id,
        e.status.clone(),
        e.psp_id.clone(),
    ) {
        Ok(Some(_)) => {
            let payments = s.payments.clone();
            tokio::spawn(async move { payments.deliver(&event).await });
            Ok(StatusCode::NO_CONTENT)
        }
        Ok(None) | Err(crate::payment::TransitionError::PaymentNotFound) => Err(AppError::NotFound),
        Err(crate::payment::TransitionError::Invalid { .. }) => Err(AppError::Conflict),
    }
}
