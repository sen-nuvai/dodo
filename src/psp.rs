use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

#[derive(Default)]
pub struct MockPsp {
    sequence: AtomicU64,
}
#[derive(Debug)]
pub enum PspError {
    Declined,
    Timeout,
    Network,
    InsufficientFunds,
}

impl PspError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Declined => "declined",
            Self::Timeout => "timeout",
            Self::Network => "network_error",
            Self::InsufficientFunds => "insufficient_funds",
        }
    }
}
#[derive(Debug)]
pub struct PspCharge {
    pub id: String,
}
#[derive(Serialize)]
struct ChargeRequest<'a> {
    amount: i64,
    currency: &'a str,
    token: &'a str,
}
#[derive(Deserialize)]
struct ChargeResponse {
    id: Option<String>,
    error: Option<String>,
}
impl MockPsp {
    pub async fn charge(
        &self,
        amount: i64,
        currency: &str,
        token: Option<&str>,
    ) -> Result<PspCharge, PspError> {
        if amount <= 0 {
            return Err(PspError::Declined);
        }
        let token = token.unwrap_or("");
        if let Ok(base) = std::env::var("PSP_URL") {
            let client = Client::builder()
                .timeout(Duration::from_secs(2))
                .build()
                .map_err(|_| PspError::Network)?;
            let response = client
                .post(format!("{base}/charges"))
                .json(&ChargeRequest {
                    amount,
                    currency,
                    token,
                })
                .send()
                .await
                .map_err(|e| {
                    if e.is_timeout() {
                        PspError::Timeout
                    } else {
                        PspError::Network
                    }
                })?;
            let body: ChargeResponse = response.json().await.map_err(|_| PspError::Network)?;
            return match body.id {
                Some(id) => Ok(PspCharge { id }),
                None => Err(match body.error.as_deref() {
                    Some("insufficient_funds") => PspError::InsufficientFunds,
                    Some("timeout") => PspError::Timeout,
                    Some("network_error") => PspError::Network,
                    _ => PspError::Declined,
                }),
            };
        }
        match token {
            "tok_success" | "" => {}
            "tok_insufficient_funds" => return Err(PspError::InsufficientFunds),
            "tok_card_declined" => return Err(PspError::Declined),
            // The timeout fixture intentionally takes 30 seconds. The payment
            // service's two-second deadline must classify it as pending without
            // making callers wait for the fixture to finish.
            "tok_timeout" => {
                tokio::time::sleep(Duration::from_secs(30)).await;
                return Err(PspError::Timeout);
            }
            "tok_network_error" => return Err(PspError::Network),
            _ => return Err(PspError::Declined),
        }
        let n = self.sequence.fetch_add(1, Ordering::Relaxed) + 1;
        Ok(PspCharge {
            id: format!("mock_ch_{n}"),
        })
    }
}
