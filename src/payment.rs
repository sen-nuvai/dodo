use crate::{
    models::payment_fingerprint,
    models::{
        Customer, Invoice, Payment, PaymentAttempt, PaymentStatus, WebhookEvent,
        WebhookRegistrationResponse,
    },
    psp::MockPsp,
};
use std::{
    collections::HashMap,
    sync::RwLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use uuid::Uuid;

pub fn webhook_retry_delay(attempt: u32) -> Duration {
    match attempt {
        1 => Duration::ZERO,
        2 => Duration::from_secs(5),
        3 => Duration::from_secs(30),
        4 => Duration::from_secs(300),
        _ => Duration::from_secs(1800),
    }
}
#[derive(Clone)]
pub struct WebhookTarget {
    pub id: Uuid,
    pub url: String,
    pub secret: String,
}
#[derive(Debug)]
pub enum TransitionError {
    PaymentNotFound,
    Invalid {
        from: PaymentStatus,
        to: PaymentStatus,
    },
}

#[derive(Default)]
pub struct PaymentStore {
    pub payments: RwLock<HashMap<Uuid, Payment>>,
    keys: RwLock<HashMap<String, (Uuid, String)>>,
    events: RwLock<HashMap<String, Uuid>>,
    pub customers: RwLock<HashMap<Uuid, Customer>>,
    pub invoices: RwLock<HashMap<Uuid, Invoice>>,
    pub attempts: RwLock<HashMap<Uuid, PaymentAttempt>>,
    pub webhooks: RwLock<HashMap<Uuid, WebhookTarget>>,
    reservation: Mutex<()>,
}
impl PaymentStore {
    pub fn get(&self, id: Uuid) -> Option<Payment> {
        self.payments.read().ok()?.get(&id).cloned()
    }
    pub fn customer(&self, id: Uuid) -> Option<Customer> {
        self.customers.read().ok()?.get(&id).cloned()
    }
    pub async fn create(
        &self,
        psp: &MockPsp,
        amount: i64,
        currency: String,
        key: Option<String>,
        fp: String,
        token: Option<&str>,
    ) -> Result<Payment, bool> {
        let _g = self.reservation.lock().await;
        self.create_locked(psp, amount, currency, key, fp, token)
            .await
    }

    async fn create_locked(
        &self,
        psp: &MockPsp,
        amount: i64,
        currency: String,
        key: Option<String>,
        fp: String,
        token: Option<&str>,
    ) -> Result<Payment, bool> {
        if let Some(k) = key.as_deref() {
            if let Some((id, old)) = self.keys.read().unwrap().get(k).cloned() {
                if old != fp {
                    return Err(true);
                };
                return Ok(self.get(id).unwrap());
            }
        }
        let id = Uuid::new_v4();
        let result =
            tokio::time::timeout(Duration::from_secs(2), psp.charge(amount, &currency, token))
                .await;
        let (status, pid, err) = match result {
            Ok(Ok(c)) => (PaymentStatus::Succeeded, Some(c.id), None),
            Ok(Err(e)) => (PaymentStatus::Failed, None, Some(format!("{e:?}"))),
            Err(_) => (
                PaymentStatus::Pending,
                None,
                Some("unknown outcome: timeout".into()),
            ),
        };
        let p = Payment {
            id,
            amount,
            currency,
            status: status.clone(),
            psp_id: pid.clone(),
            idempotency_key: key.clone(),
            attempts: 1,
        };
        self.payments.write().unwrap().insert(id, p.clone());
        let aid = Uuid::new_v4();
        self.attempts.write().unwrap().insert(
            aid,
            PaymentAttempt {
                id: aid,
                payment_id: id,
                status,
                psp_id: pid,
                error: err,
            },
        );
        if let Some(k) = key {
            self.keys.write().unwrap().insert(k, (id, fp));
        }
        Ok(p)
    }
    pub fn insert_customer(&self, c: Customer) {
        self.customers.write().unwrap().insert(c.id, c);
    }
    pub fn insert_invoice(&self, i: Invoice) {
        self.invoices.write().unwrap().insert(i.id, i);
    }
    pub async fn finalize_invoice(&self, id: Uuid) -> Option<Invoice> {
        let _g = self.reservation.lock().await;
        let mut i = self.invoice(id)?;
        if i.status != "draft" {
            return None;
        }
        i.status = "open".into();
        self.insert_invoice(i.clone());
        Some(i)
    }

    pub async fn pay_invoice(
        &self,
        psp: &MockPsp,
        id: Uuid,
        key: Option<String>,
        token: &str,
    ) -> Result<Invoice, bool> {
        let _g = self.reservation.lock().await;
        let mut i = self.invoice(id).ok_or(false)?;
        if i.status == "paid" {
            return Ok(i);
        }
        if i.status != "open" {
            return Err(true);
        }
        let fp = payment_fingerprint(id, token, i.amount, &i.currency);
        let p = self
            .create_locked(psp, i.amount, i.currency.clone(), key, fp, Some(token))
            .await?;
        i.payment_id = Some(p.id);
        i.status = match p.status {
            PaymentStatus::Succeeded => "paid",
            PaymentStatus::Pending | PaymentStatus::Failed => "open",
        }
        .into();
        self.insert_invoice(i.clone());
        Ok(i)
    }
    pub fn invoice(&self, id: Uuid) -> Option<Invoice> {
        self.invoices.read().ok()?.get(&id).cloned()
    }
    pub fn invoices(&self, status: Option<&str>, customer: Option<Uuid>) -> Vec<Invoice> {
        self.invoices
            .read()
            .map(|m| {
                m.values()
                    .filter(|i| {
                        status.is_none_or(|s| i.status == s)
                            && customer.is_none_or(|c| i.customer_id == c)
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
    pub fn update_customer(
        &self,
        id: Uuid,
        email: Option<String>,
        name: Option<String>,
    ) -> Option<Customer> {
        let mut a = self.customers.write().ok()?;
        let c = a.get_mut(&id)?;
        if let Some(v) = email {
            if v.trim().is_empty() {
                return None;
            }
            c.email = v;
        }
        if let Some(v) = name {
            c.name = v;
        }
        Some(c.clone())
    }
    pub fn attempts(&self, pid: Uuid) -> Vec<PaymentAttempt> {
        self.attempts
            .read()
            .map(|m| {
                m.values()
                    .filter(|a| a.payment_id == pid)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
    pub fn apply_event(
        &self,
        eid: String,
        pid: Uuid,
        status: PaymentStatus,
        psp: Option<String>,
    ) -> Result<Option<Payment>, TransitionError> {
        let mut ev = self.events.write().unwrap();
        if ev.contains_key(&eid) {
            return Ok(self.get(pid));
        }
        let mut a = self.payments.write().unwrap();
        let p = a.get_mut(&pid).ok_or(TransitionError::PaymentNotFound)?;
        // The only legal transition is pending -> one terminal state. Repeated
        // events are handled above by event ID, while contradictory events are
        // rejected rather than silently changing a terminal payment.
        if !matches!(
            (&p.status, &status),
            (
                PaymentStatus::Pending,
                PaymentStatus::Succeeded | PaymentStatus::Failed
            )
        ) {
            return Err(TransitionError::Invalid {
                from: p.status.clone(),
                to: status,
            });
        }
        p.status = status;
        p.psp_id = psp;
        ev.insert(eid, pid);
        Ok(Some(p.clone()))
    }
    pub fn register(&self, url: String) -> WebhookRegistrationResponse {
        let id = Uuid::new_v4();
        let secret = Uuid::new_v4().to_string();
        self.webhooks.write().unwrap().insert(
            id,
            WebhookTarget {
                id,
                url: url.clone(),
                secret: secret.clone(),
            },
        );
        WebhookRegistrationResponse { id, url, secret }
    }
    pub async fn deliver(&self, event: &WebhookEvent) {
        let body = match serde_json::to_vec(event) {
            Ok(v) => v,
            Err(_) => return,
        };
        let targets = self
            .webhooks
            .read()
            .unwrap()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .expect("valid webhook client");
        for t in targets {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let sig = crate::models::webhook_signature(&t.secret, timestamp as i64, &body);
            for n in 0..5 {
                let result = client
                    .post(&t.url)
                    .header("x-webhook-timestamp", timestamp.to_string())
                    .header("x-webhook-signature", &sig)
                    .header("content-type", "application/json")
                    .body(body.clone())
                    .send()
                    .await;
                if result.is_ok_and(|r| r.status().is_success()) {
                    break;
                }
                tokio::time::sleep(webhook_retry_delay(n + 1)).await;
            }
        }
    }
}
