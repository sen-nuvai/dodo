use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub fn canonical_fingerprint<T: Serialize>(value: &T) -> String {
    let json = serde_json::to_value(value).unwrap_or(serde_json::Value::Null);
    fn normalize(v: serde_json::Value) -> serde_json::Value {
        match v {
            serde_json::Value::Object(m) => {
                let mut entries: Vec<_> = m.into_iter().collect();
                entries.sort_by(|a, b| a.0.cmp(&b.0));
                serde_json::Value::Object(
                    entries
                        .into_iter()
                        .map(|(k, v)| (k, normalize(v)))
                        .collect(),
                )
            }
            serde_json::Value::Array(a) => {
                serde_json::Value::Array(a.into_iter().map(normalize).collect())
            }
            v => v,
        }
    }
    hex::encode(Sha256::digest(
        serde_json::to_vec(&normalize(json)).unwrap_or_default(),
    ))
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InvoiceStatus {
    Draft,
    Open,
    Paid,
    Void,
    Uncollectible,
}
impl InvoiceStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Open => "open",
            Self::Paid => "paid",
            Self::Void => "void",
            Self::Uncollectible => "uncollectible",
        }
    }
}

/// Versioned, length-delimited request fingerprint. The token is never persisted.
pub fn payment_fingerprint(invoice_id: Uuid, token: &str, amount: i64, currency: &str) -> String {
    let mut h = Sha256::new();
    h.update(b"dodo-payment-v1\0");
    for part in [
        invoice_id.to_string(),
        token.to_owned(),
        amount.to_string(),
        currency.to_uppercase(),
    ] {
        h.update((part.len() as u64).to_be_bytes());
        h.update(part.as_bytes());
    }
    hex::encode(h.finalize())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaymentStatus {
    Pending,
    Succeeded,
    Failed,
}
impl PaymentStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Payment {
    pub id: Uuid,
    pub amount: i64,
    pub currency: String,
    pub status: PaymentStatus,
    pub psp_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub attempts: u32,
}
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CreatePayment {
    pub amount: Option<i64>,
    pub currency: String,
    pub line_items: Option<Vec<LineItem>>,
    pub payment_method_token: Option<String>,
}
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LineItem {
    pub quantity: i64,
    pub unit_amount_cents: i64,
    pub description: Option<String>,
}
impl CreatePayment {
    pub fn total(&self) -> Result<i64, String> {
        if let Some(a) = self.amount {
            if a <= 0 {
                return Err("amount must be positive".into());
            };
            return Ok(a);
        }
        let xs = self
            .line_items
            .as_ref()
            .ok_or("amount or line_items required")?;
        if xs.is_empty() {
            return Err("line_items cannot be empty".into());
        };
        xs.iter().try_fold(0i64, |acc, x| {
            if x.quantity <= 0 || x.unit_amount_cents <= 0 {
                return Err("line item values must be positive".into());
            };
            acc.checked_add(
                x.quantity
                    .checked_mul(x.unit_amount_cents)
                    .ok_or("line item overflow")?,
            )
            .ok_or("total overflow".into())
        })
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Customer {
    pub id: Uuid,
    pub email: String,
    pub name: String,
}
#[derive(Debug, Deserialize)]
pub struct CreateCustomer {
    pub email: String,
    pub name: String,
}
#[derive(Debug, Deserialize)]
pub struct UpdateCustomer {
    pub email: Option<String>,
    pub name: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Invoice {
    pub id: Uuid,
    pub customer_id: Uuid,
    pub amount: i64,
    pub currency: String,
    pub status: String,
    pub payment_id: Option<Uuid>,
    pub due_date: Option<chrono::NaiveDate>,
    pub line_items: Vec<LineItem>,
}
#[derive(Debug, Deserialize)]
pub struct CreateInvoice {
    pub customer_id: Uuid,
    pub currency: String,
    pub line_items: Vec<LineItem>,
    pub due_date: Option<chrono::NaiveDate>,
}
#[derive(Debug, Deserialize)]
pub struct InvoiceFilter {
    pub status: Option<String>,
    pub customer_id: Option<Uuid>,
}
#[derive(Debug, Deserialize)]
pub struct PayInvoice {
    pub payment_method_token: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PaymentAttempt {
    pub id: Uuid,
    pub payment_id: Uuid,
    pub status: PaymentStatus,
    pub psp_id: Option<String>,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WebhookEvent {
    pub event_id: String,
    pub payment_id: Uuid,
    pub status: PaymentStatus,
    pub psp_id: Option<String>,
}
#[derive(Debug, Deserialize)]
pub struct WebhookRegistration {
    pub url: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct WebhookRegistrationResponse {
    pub id: Uuid,
    pub url: String,
    pub secret: String,
}
