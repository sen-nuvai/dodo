use crate::models::LineItem;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::time::Duration;
use uuid::Uuid;
type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
pub struct PgRepository {
    pub pool: PgPool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPayment {
    pub id: Uuid,
    pub amount: i64,
    pub currency: String,
    pub status: String,
    pub provider_id: Option<String>,
    pub attempts: i32,
}
#[derive(Debug, Clone)]
pub struct StoredInvoice {
    pub id: Uuid,
    pub customer_id: Uuid,
    pub amount: i64,
    pub currency: String,
    pub status: String,
    pub payment_id: Option<Uuid>,
    pub due_date: Option<chrono::NaiveDate>,
    pub line_items: Vec<LineItem>,
}
#[derive(Debug, Clone)]
pub struct StoredWebhook {
    pub id: Uuid,
    pub url: String,
    pub secret: String,
}
#[derive(Debug)]
pub enum RepositoryError {
    Database(sqlx::Error),
    IdempotencyConflict,
    NotFound,
}

impl From<sqlx::Error> for RepositoryError {
    fn from(e: sqlx::Error) -> Self {
        Self::Database(e)
    }
}

impl PgRepository {
    pub async fn connect(url: &str) -> Result<Self, sqlx::Error> {
        Ok(Self {
            pool: PgPoolOptions::new()
                .max_connections(10)
                .connect(url)
                .await?,
        })
    }
    pub async fn migrate(&self) -> Result<(), sqlx::migrate::MigrateError> {
        sqlx::migrate!("./migrations").run(&self.pool).await
    }
    pub async fn authenticate_api_key(&self, key: &str) -> Result<Option<Uuid>, sqlx::Error> {
        let (prefix, secret) = key.split_once('_').unwrap_or(("", "")).to_owned();
        if prefix.is_empty() || secret.is_empty() {
            return Ok(None);
        }
        let hash = hex::encode(Sha256::digest(secret.as_bytes()));
        sqlx::query_scalar("SELECT id FROM tenants WHERE api_key_prefix=$1 AND api_key_hash=$2")
            .bind(prefix)
            .bind(hash)
            .fetch_optional(&self.pool)
            .await
    }
    pub async fn bootstrap_tenant(&self, name: &str, key: &str) -> Result<Uuid, sqlx::Error> {
        let (prefix, secret) = key
            .split_once('_')
            .ok_or_else(|| sqlx::Error::Protocol("bootstrap key must be prefix_secret".into()))?;
        if prefix.is_empty() || secret.is_empty() {
            return Err(sqlx::Error::Protocol(
                "bootstrap key must be prefix_secret".into(),
            ));
        }
        let hash = hex::encode(Sha256::digest(secret.as_bytes()));
        if let Some(id) =
            sqlx::query_scalar::<_, Uuid>("SELECT id FROM tenants WHERE api_key_prefix=$1")
                .bind(prefix)
                .fetch_optional(&self.pool)
                .await?
        {
            sqlx::query(
                "UPDATE tenants SET name=$1, api_key_prefix=$2, api_key_hash=$3 WHERE id=$4",
            )
            .bind(name)
            .bind(prefix)
            .bind(hash)
            .bind(id)
            .execute(&self.pool)
            .await?;
            return Ok(id);
        }
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO tenants(id,name,api_key_prefix,api_key_hash) VALUES($1,$2,$3,$4)")
            .bind(id)
            .bind(name)
            .bind(prefix)
            .bind(hash)
            .execute(&self.pool)
            .await?;
        Ok(id)
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn create_payment(
        &self,
        tenant: Uuid,
        amount: i64,
        currency: &str,
        key: Option<&str>,
        fp: &str,
        provider: Option<&str>,
        status: &str,
    ) -> Result<StoredPayment, RepositoryError> {
        let mut tx = self.pool.begin().await?;
        if let Some(k) = key {
            // PostgreSQL unique constraints reject concurrent inserts, but the
            // transaction would then be aborted before we can replay the row.
            // Serialize the key's lookup/insert with a transaction advisory lock.
            sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
                .bind(format!("{tenant}:{k}"))
                .execute(&mut *tx)
                .await?;
        }
        let old = if let Some(k) = key {
            sqlx::query_as::<_,(Uuid,String,String,i64,String,Option<String>,i32)>("SELECT id,request_fingerprint,status,amount,currency,provider_id,attempts FROM payments WHERE tenant_id=$1 AND idempotency_key=$2 FOR UPDATE").bind(tenant).bind(k).fetch_optional(&mut *tx).await?
        } else {
            None
        };
        if let Some((id, oldfp, st, amt, cur, pid, att)) = old {
            if oldfp != fp {
                return Err(RepositoryError::IdempotencyConflict);
            }
            tx.commit().await?;
            return Ok(StoredPayment {
                id,
                amount: amt,
                currency: cur,
                status: st,
                provider_id: pid,
                attempts: att,
            });
        }
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO payments(id,tenant_id,amount,currency,status,provider,provider_id,idempotency_key,request_fingerprint,attempts) VALUES($1,$2,$3,$4,$5,'mock',$6,$7,$8,1)").bind(id).bind(tenant).bind(amount).bind(currency).bind(status).bind(provider).bind(key).bind(fp).execute(&mut *tx).await?;
        sqlx::query(
            "INSERT INTO payment_attempts(id,payment_id,status,provider_id) VALUES($1,$2,$3,$4)",
        )
        .bind(Uuid::new_v4())
        .bind(id)
        .bind(status)
        .bind(provider)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(StoredPayment {
            id,
            amount,
            currency: currency.into(),
            status: status.into(),
            provider_id: provider.map(str::to_owned),
            attempts: 1,
        })
    }
    pub async fn get_payment(
        &self,
        tenant: Uuid,
        id: Uuid,
    ) -> Result<Option<StoredPayment>, RepositoryError> {
        Ok(sqlx::query_as::<_,(Uuid,i64,String,String,Option<String>,i32)>("SELECT id,amount,currency,status,provider_id,attempts FROM payments WHERE tenant_id=$1 AND id=$2").bind(tenant).bind(id).fetch_optional(&self.pool).await?.map(|(id,amount,currency,status,provider_id,attempts)|StoredPayment{id,amount,currency,status,provider_id,attempts}))
    }
    pub async fn list_attempts(
        &self,
        tenant: Uuid,
        pid: Uuid,
    ) -> Result<Vec<(Uuid, Uuid, String, Option<String>, Option<String>)>, RepositoryError> {
        Ok(sqlx::query_as("SELECT a.id,a.payment_id,a.status,a.provider_id,a.error FROM payment_attempts a JOIN payments p ON p.id=a.payment_id WHERE p.tenant_id=$1 AND p.id=$2 ORDER BY a.created_at").bind(tenant).bind(pid).fetch_all(&self.pool).await?)
    }
    pub async fn create_customer(
        &self,
        t: Uuid,
        email: &str,
        name: &str,
    ) -> Result<(Uuid, String, String), RepositoryError> {
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO customers(id,tenant_id,email,name) VALUES($1,$2,$3,$4)")
            .bind(id)
            .bind(t)
            .bind(email)
            .bind(name)
            .execute(&self.pool)
            .await?;
        Ok((id, email.into(), name.into()))
    }
    pub async fn get_customer(
        &self,
        t: Uuid,
        id: Uuid,
    ) -> Result<Option<(Uuid, String, String)>, RepositoryError> {
        Ok(
            sqlx::query_as("SELECT id,email,name FROM customers WHERE tenant_id=$1 AND id=$2")
                .bind(t)
                .bind(id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }
    pub async fn list_customers(
        &self,
        t: Uuid,
    ) -> Result<Vec<(Uuid, String, String)>, RepositoryError> {
        Ok(
            sqlx::query_as("SELECT id,email,name FROM customers WHERE tenant_id=$1 ORDER BY id")
                .bind(t)
                .fetch_all(&self.pool)
                .await?,
        )
    }
    pub async fn update_customer(
        &self,
        t: Uuid,
        id: Uuid,
        email: Option<&str>,
        name: Option<&str>,
    ) -> Result<Option<(Uuid, String, String)>, RepositoryError> {
        Ok(sqlx::query_as("UPDATE customers SET email=COALESCE($3,email),name=COALESCE($4,name) WHERE tenant_id=$1 AND id=$2 RETURNING id,email,name").bind(t).bind(id).bind(email).bind(name).fetch_optional(&self.pool).await?)
    }
    pub async fn create_invoice(
        &self,
        t: Uuid,
        cid: Uuid,
        amount: i64,
        currency: &str,
        due: Option<chrono::NaiveDate>,
        items: &[LineItem],
    ) -> Result<StoredInvoice, RepositoryError> {
        let mut tx = self.pool.begin().await?;
        // The customer must belong to the same tenant. The foreign key only
        // protects existence, not tenant isolation.
        let exists: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM customers WHERE id=$1 AND tenant_id=$2")
                .bind(cid)
                .bind(t)
                .fetch_optional(&mut *tx)
                .await?;
        if exists.is_none() {
            return Err(RepositoryError::NotFound);
        }
        let id = Uuid::new_v4();
        let json = serde_json::to_value(items)
            .map_err(|_| sqlx::Error::Protocol("invalid items".into()))?;
        sqlx::query("INSERT INTO invoices(id,tenant_id,customer_id,amount,currency,status,due_date,line_items) VALUES($1,$2,$3,$4,$5,'draft',$6,$7)").bind(id).bind(t).bind(cid).bind(amount).bind(currency).bind(due).bind(json).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(StoredInvoice {
            id,
            customer_id: cid,
            amount,
            currency: currency.into(),
            status: "draft".into(),
            payment_id: None,
            due_date: due,
            line_items: items.to_vec(),
        })
    }
    async fn invoice_row(
        &self,
        t: Uuid,
        id: Uuid,
        lock: bool,
    ) -> Result<Option<StoredInvoice>, RepositoryError> {
        let q=format!("SELECT id,customer_id,amount,currency,status,payment_id,due_date,line_items FROM invoices WHERE tenant_id=$1 AND id=$2{}",if lock{" FOR UPDATE"}else{""});
        let r = sqlx::query_as::<
            _,
            (
                Uuid,
                Uuid,
                i64,
                String,
                String,
                Option<Uuid>,
                Option<chrono::NaiveDate>,
                serde_json::Value,
            ),
        >(&q)
        .bind(t)
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(r.map(
            |(id, customer_id, amount, currency, status, payment_id, due_date, v)| StoredInvoice {
                id,
                customer_id,
                amount,
                currency,
                status,
                payment_id,
                due_date,
                line_items: serde_json::from_value(v).unwrap_or_default(),
            },
        ))
    }
    pub async fn get_invoice(
        &self,
        t: Uuid,
        id: Uuid,
    ) -> Result<Option<StoredInvoice>, RepositoryError> {
        self.invoice_row(t, id, false).await
    }
    pub async fn list_invoices(
        &self,
        t: Uuid,
        status: Option<&str>,
        cid: Option<Uuid>,
    ) -> Result<Vec<StoredInvoice>, RepositoryError> {
        let rows=sqlx::query_as::<_,(Uuid,Uuid,i64,String,String,Option<Uuid>,Option<chrono::NaiveDate>,serde_json::Value)>("SELECT id,customer_id,amount,currency,status,payment_id,due_date,line_items FROM invoices WHERE tenant_id=$1 AND ($2::text IS NULL OR status=$2) AND ($3::uuid IS NULL OR customer_id=$3) ORDER BY id").bind(t).bind(status).bind(cid).fetch_all(&self.pool).await?;
        Ok(rows
            .into_iter()
            .map(
                |(id, customer_id, amount, currency, status, payment_id, due_date, v)| {
                    StoredInvoice {
                        id,
                        customer_id,
                        amount,
                        currency,
                        status,
                        payment_id,
                        due_date,
                        line_items: serde_json::from_value(v).unwrap_or_default(),
                    }
                },
            )
            .collect())
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn pay_invoice(
        &self,
        t: Uuid,
        id: Uuid,
        key: Option<&str>,
        fp: &str,
        status: &str,
        provider: Option<&str>,
        error: Option<&str>,
    ) -> Result<StoredInvoice, RepositoryError> {
        let mut tx = self.pool.begin().await?;
        let row=sqlx::query_as::<_,(Uuid,Uuid,i64,String,String,Option<Uuid>,Option<chrono::NaiveDate>,serde_json::Value)>("SELECT id,customer_id,amount,currency,status,payment_id,due_date,line_items FROM invoices WHERE tenant_id=$1 AND id=$2 FOR UPDATE").bind(t).bind(id).fetch_optional(&mut *tx).await? .ok_or(RepositoryError::NotFound)?;
        let (iid, cid, amount, currency, old, payment_id, due, items) = row;
        if old == "paid" {
            tx.commit().await?;
            return Ok(StoredInvoice {
                id: iid,
                customer_id: cid,
                amount,
                currency,
                status: old,
                payment_id,
                due_date: due,
                line_items: serde_json::from_value(items).unwrap_or_default(),
            });
        }
        if old != "draft" {
            return Err(RepositoryError::IdempotencyConflict);
        }
        if let Some(k) = key {
            let existing = sqlx::query_as::<_, (Uuid, String, String, i64, String, Option<String>, i32)>(
                "SELECT id,status,currency,amount,request_fingerprint,provider_id,attempts FROM payments WHERE tenant_id=$1 AND idempotency_key=$2",
            )
            .bind(t)
            .bind(k)
            .fetch_optional(&mut *tx)
            .await?;
            if let Some((_, _, _, _, old_fp, _, _)) = existing {
                if old_fp != fp {
                    return Err(RepositoryError::IdempotencyConflict);
                }
                return Err(RepositoryError::IdempotencyConflict);
            }
        }
        let payment_id = Uuid::new_v4();
        let attempts = 1;
        sqlx::query("INSERT INTO payments(id,tenant_id,amount,currency,status,provider,provider_id,idempotency_key,request_fingerprint,attempts) VALUES($1,$2,$3,$4,$5,'mock',$6,$7,$8,$9)")
            .bind(payment_id).bind(t).bind(amount).bind(&currency).bind(status).bind(provider).bind(key).bind(fp).bind(attempts)
            .execute(&mut *tx).await?;
        sqlx::query(
            "INSERT INTO payment_attempts(id,payment_id,status,provider_id,error) VALUES($1,$2,$3,$4,$5)",
        )
        .bind(Uuid::new_v4())
        .bind(payment_id)
        .bind(status)
        .bind(provider)
        .bind(error)
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE invoices SET status=$1,payment_id=$2 WHERE tenant_id=$3 AND id=$4")
            .bind(match status {
                "succeeded" => "paid",
                "failed" => "failed",
                _ => "pending",
            })
            .bind(payment_id)
            .bind(t)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(StoredInvoice {
            id: iid,
            customer_id: cid,
            amount,
            currency,
            status: match status {
                "succeeded" => "paid".into(),
                "failed" => "failed".into(),
                _ => "pending".into(),
            },
            payment_id: Some(payment_id),
            due_date: due,
            line_items: serde_json::from_value(items).unwrap_or_default(),
        })
    }
    pub async fn register_webhook(
        &self,
        t: Uuid,
        url: &str,
    ) -> Result<StoredWebhook, RepositoryError> {
        let id = Uuid::new_v4();
        let secret = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO webhook_registrations(id,tenant_id,url,secret) VALUES($1,$2,$3,$4)",
        )
        .bind(id)
        .bind(t)
        .bind(url)
        .bind(&secret)
        .execute(&self.pool)
        .await?;
        Ok(StoredWebhook {
            id,
            url: url.into(),
            secret,
        })
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn apply_webhook(
        &self,
        t: Uuid,
        event_id: &str,
        pid: Uuid,
        status: &str,
        provider: Option<&str>,
        raw: &[u8],
        signature: &str,
        registration_id: Option<Uuid>,
    ) -> Result<bool, RepositoryError> {
        let mut tx = self.pool.begin().await?;
        let registrations = sqlx::query_as::<_, (Uuid, String)>(
            "SELECT id,secret FROM webhook_registrations WHERE tenant_id=$1 AND active=true AND ($2::uuid IS NULL OR id=$2)",
        )
        .bind(t)
        .bind(registration_id)
        .fetch_all(&mut *tx)
        .await?;
        let registration_id = registrations
            .into_iter()
            .find_map(|(id, secret)| {
                let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).ok()?;
                mac.update(raw);
                let expected = hex::encode(mac.finalize().into_bytes());
                (expected == signature).then_some(id)
            })
            .ok_or_else(|| sqlx::Error::Protocol("invalid webhook signature".into()))?;
        let _: serde_json::Value = serde_json::from_slice(raw)
            .map_err(|_| sqlx::Error::Protocol("invalid webhook payload".into()))?;
        let inserted = sqlx::query("INSERT INTO webhook_events(id,registration_id,event_id,payload) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING")
            .bind(Uuid::new_v4()).bind(registration_id).bind(event_id).bind(raw).execute(&mut *tx).await?;
        if inserted.rows_affected() == 0 {
            tx.commit().await?;
            return Ok(true);
        }
        sqlx::query("INSERT INTO webhook_deliveries(id,registration_id,event_id,payload) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING")
            .bind(Uuid::new_v4()).bind(registration_id).bind(event_id).bind(raw).execute(&mut *tx).await?;
        let changed = sqlx::query("UPDATE payments SET status=$1,provider_id=COALESCE($2,provider_id),updated_at=now() WHERE id=$3 AND tenant_id=$4 AND status='pending'")
            .bind(status).bind(provider).bind(pid).bind(t).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(changed.rows_affected() == 1)
    }
}
pub async fn run_delivery_worker(repo: PgRepository) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("client");
    loop {
        let rows = sqlx::query_as::<_,(Uuid,String,String,Vec<u8>)>("UPDATE webhook_deliveries SET attempts=attempts+1,next_attempt_at=now()+least((2^LEAST(attempts,8)) * interval '1 second', interval '1 hour') WHERE id IN (SELECT id FROM webhook_deliveries WHERE delivered_at IS NULL AND attempts < max_attempts AND next_attempt_at<=now() ORDER BY next_attempt_at FOR UPDATE SKIP LOCKED LIMIT 20) RETURNING id,(SELECT url FROM webhook_registrations r WHERE r.id=registration_id),(SELECT secret FROM webhook_registrations r WHERE r.id=registration_id),payload").fetch_all(&repo.pool).await.unwrap_or_default();
        for (id, url, secret, payload) in rows {
            let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("hmac");
            mac.update(&payload);
            let result = client
                .post(url)
                .header(
                    "x-webhook-signature",
                    hex::encode(mac.finalize().into_bytes()),
                )
                .body(payload)
                .send()
                .await;
            match result {
                Ok(r) if r.status().is_success() => {
                    let _ =
                        sqlx::query("UPDATE webhook_deliveries SET delivered_at=now() WHERE id=$1")
                            .bind(id)
                            .execute(&repo.pool)
                            .await;
                }
                Ok(r) => {
                    let _ = sqlx::query("UPDATE webhook_deliveries SET last_error=$2 WHERE id=$1")
                        .bind(id)
                        .bind(format!("http {}", r.status()))
                        .execute(&repo.pool)
                        .await;
                }
                Err(e) => {
                    let _ = sqlx::query("UPDATE webhook_deliveries SET last_error=$2 WHERE id=$1")
                        .bind(id)
                        .bind(e.to_string())
                        .execute(&repo.pool)
                        .await;
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
