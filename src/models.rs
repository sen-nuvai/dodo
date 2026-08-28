use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
