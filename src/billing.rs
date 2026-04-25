use crate::models::{self, plan_type, subscription_status, Subscription};
use crate::repos::{SubscriptionRepository, UserRepository};
use anyhow::{anyhow, Result};
use chrono::{DateTime, TimeZone, Utc};
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value as JsonValue;
use sha2::Sha256;
use sqlx::{PgPool, Postgres, Transaction};
use std::sync::Arc;
use subtle::ConstantTimeEq;
use uuid::Uuid;

const STRIPE_API: &str = "https://api.stripe.com/v1";

pub struct BillingService {
    pool: PgPool,
    user_repo: Arc<UserRepository>,
    subscription_repo: Arc<SubscriptionRepository>,
    app_url: String,
    pub stripe_secret_key: String,
    pub stripe_webhook_secret: String,
    pub starter_price_id: String,
    pub pro_price_id: String,
    pub family_price_id: String,
    pub extra_cinematic_price_id: String,
    http: Client,
}

#[derive(Debug, thiserror::Error)]
pub enum BillingError {
    #[error("billing is not configured")]
    NotConfigured,
    #[error("invalid billing plan")]
    InvalidPlan,
    #[error("no stripe customer on file")]
    NoStripeCustomer,
    #[error("an active paid plan is required for this purchase")]
    PaidPlanRequired,
    #[error("entitlement: {code}: {message}")]
    Entitlement { code: String, message: String },
    #[error("other: {0}")]
    Other(#[from] anyhow::Error),
}

impl BillingService {
    pub fn new(
        pool: PgPool,
        user_repo: Arc<UserRepository>,
        subscription_repo: Arc<SubscriptionRepository>,
        app_url: String,
        stripe_secret_key: String,
        stripe_webhook_secret: String,
        starter_price_id: String,
        pro_price_id: String,
        family_price_id: String,
        extra_cinematic_price_id: String,
    ) -> Self {
        Self {
            pool,
            user_repo,
            subscription_repo,
            app_url,
            stripe_secret_key,
            stripe_webhook_secret,
            starter_price_id,
            pro_price_id,
            family_price_id,
            extra_cinematic_price_id,
            http: Client::new(),
        }
    }

    pub fn billing_enabled(&self) -> bool {
        !self.stripe_secret_key.is_empty()
    }

    fn plan_checkout_configured(&self) -> bool {
        self.billing_enabled()
            && !self.starter_price_id.is_empty()
            && !self.pro_price_id.is_empty()
            && !self.family_price_id.is_empty()
    }

    fn extra_checkout_configured(&self) -> bool {
        self.plan_checkout_configured() && !self.extra_cinematic_price_id.is_empty()
    }

    fn webhook_configured(&self) -> bool {
        self.billing_enabled() && !self.stripe_webhook_secret.is_empty()
    }

    fn price_id_for_plan(&self, plan: &str) -> Result<&str, BillingError> {
        match plan {
            plan_type::EXPLORER | plan_type::STARTER => {
                if self.starter_price_id.is_empty() {
                    Err(BillingError::NotConfigured)
                } else {
                    Ok(&self.starter_price_id)
                }
            }
            plan_type::PRO => {
                if self.pro_price_id.is_empty() {
                    Err(BillingError::NotConfigured)
                } else {
                    Ok(&self.pro_price_id)
                }
            }
            plan_type::UNLIMITED | plan_type::FAMILY => {
                if self.family_price_id.is_empty() {
                    Err(BillingError::NotConfigured)
                } else {
                    Ok(&self.family_price_id)
                }
            }
            _ => Err(BillingError::InvalidPlan),
        }
    }

    pub async fn get_subscription(&self, user_id: Uuid) -> Result<Subscription> {
        self.subscription_repo
            .ensure_free_subscription(user_id)
            .await
    }

    pub async fn check_cinematic_entitlement(&self, user_id: Uuid) -> Result<(), BillingError> {
        if !self.billing_enabled() {
            return Ok(());
        }
        self.subscription_repo
            .check_cinematic_entitlement(user_id)
            .await
            .map_err(|e| BillingError::Entitlement {
                code: e.code,
                message: e.message,
            })
    }

    pub async fn create_checkout_session(
        &self,
        user_id: Uuid,
        plan: &str,
    ) -> Result<String, BillingError> {
        if !self.plan_checkout_configured() {
            return Err(BillingError::NotConfigured);
        }
        let price_id = self.price_id_for_plan(plan)?.to_string();
        let customer_id = self.ensure_stripe_customer(user_id).await?;

        #[derive(Deserialize)]
        struct Session {
            url: String,
        }
        let resp: Session = self
            .stripe_post(
                "/checkout/sessions",
                vec![
                    ("mode", "subscription".into()),
                    ("customer", customer_id),
                    ("client_reference_id", user_id.to_string()),
                    (
                        "success_url",
                        format!(
                            "{}/billing/success?kind=subscription&plan={}",
                            self.app_url, plan
                        ),
                    ),
                    (
                        "cancel_url",
                        format!("{}/billing?checkout=canceled&plan={}", self.app_url, plan),
                    ),
                    ("line_items[0][price]", price_id),
                    ("line_items[0][quantity]", "1".into()),
                    ("metadata[user_id]", user_id.to_string()),
                    ("metadata[plan]", plan.to_string()),
                    ("metadata[purchase_kind]", "subscription".into()),
                    ("subscription_data[metadata][user_id]", user_id.to_string()),
                    ("subscription_data[metadata][plan]", plan.to_string()),
                ],
            )
            .await
            .map_err(BillingError::Other)?;
        Ok(resp.url)
    }

    pub async fn create_extra_cinematic_checkout_session(
        &self,
        user_id: Uuid,
    ) -> Result<String, BillingError> {
        if !self.extra_checkout_configured() {
            return Err(BillingError::NotConfigured);
        }
        let sub = self
            .subscription_repo
            .ensure_free_subscription(user_id)
            .await
            .map_err(|e| BillingError::Other(anyhow!(e)))?;
        if sub.plan == plan_type::FREE || !sub.billing_active() {
            return Err(BillingError::PaidPlanRequired);
        }
        let customer_id = self.ensure_stripe_customer(user_id).await?;

        #[derive(Deserialize)]
        struct Session {
            url: String,
        }
        let resp: Session = self
            .stripe_post(
                "/checkout/sessions",
                vec![
                    ("mode", "payment".into()),
                    ("customer", customer_id),
                    ("client_reference_id", user_id.to_string()),
                    (
                        "success_url",
                        format!("{}/billing/success?kind=extra-cinematic", self.app_url),
                    ),
                    (
                        "cancel_url",
                        format!(
                            "{}/billing?checkout=canceled&kind=extra-cinematic",
                            self.app_url
                        ),
                    ),
                    (
                        "line_items[0][price]",
                        self.extra_cinematic_price_id.clone(),
                    ),
                    ("line_items[0][quantity]", "1".into()),
                    ("metadata[user_id]", user_id.to_string()),
                    ("metadata[credits]", "1".into()),
                    ("metadata[purchase_kind]", "extra_cinematic".into()),
                    (
                        "payment_intent_data[metadata][user_id]",
                        user_id.to_string(),
                    ),
                    (
                        "payment_intent_data[metadata][purchase_kind]",
                        "extra_cinematic".into(),
                    ),
                ],
            )
            .await
            .map_err(BillingError::Other)?;
        Ok(resp.url)
    }

    pub async fn create_portal_session(&self, user_id: Uuid) -> Result<String, BillingError> {
        if !self.billing_enabled() {
            return Err(BillingError::NotConfigured);
        }
        let sub = self
            .subscription_repo
            .ensure_free_subscription(user_id)
            .await
            .map_err(|e| BillingError::Other(anyhow!(e)))?;
        let customer_id = sub
            .stripe_customer_id
            .clone()
            .filter(|s| !s.is_empty())
            .ok_or(BillingError::NoStripeCustomer)?;

        #[derive(Deserialize)]
        struct Session {
            url: String,
        }
        let resp: Session = self
            .stripe_post(
                "/billing_portal/sessions",
                vec![
                    ("customer", customer_id),
                    ("return_url", format!("{}/billing", self.app_url)),
                ],
            )
            .await
            .map_err(BillingError::Other)?;
        Ok(resp.url)
    }

    pub async fn handle_webhook(
        &self,
        payload: &[u8],
        signature: &str,
    ) -> Result<(), BillingError> {
        if !self.webhook_configured() {
            return Err(BillingError::NotConfigured);
        }
        let event = verify_stripe_signature(&self.stripe_webhook_secret, signature, payload)
            .map_err(BillingError::Other)?;

        let event_id = event
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BillingError::Other(anyhow!("no event id")))?
            .to_string();
        let event_type = event
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BillingError::Other(anyhow!("no event type")))?
            .to_string();

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| BillingError::Other(anyhow!(e)))?;

        // Idempotency: skip if already processed.
        let exists: (bool,) = sqlx::query_as(
            "SELECT EXISTS(SELECT 1 FROM stripe_webhook_events WHERE event_id = $1)",
        )
        .bind(&event_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| BillingError::Other(anyhow!(e)))?;
        if exists.0 {
            tx.commit()
                .await
                .map_err(|e| BillingError::Other(anyhow!(e)))?;
            return Ok(());
        }

        let obj = event
            .get("data")
            .and_then(|d| d.get("object"))
            .cloned()
            .unwrap_or(JsonValue::Null);

        match event_type.as_str() {
            "checkout.session.completed" => {
                self.handle_checkout_completed_tx(&mut tx, &obj).await?;
            }
            "customer.subscription.created" | "customer.subscription.updated" => {
                self.handle_subscription_updated_tx(&mut tx, &obj).await?;
            }
            "customer.subscription.deleted" => {
                if let Some(sub_id) = obj.get("id").and_then(|v| v.as_str()) {
                    if !sub_id.is_empty() {
                        self.subscription_repo
                            .cancel_subscription_tx(&mut tx, sub_id)
                            .await
                            .map_err(|e| BillingError::Other(anyhow!(e)))?;
                    }
                }
            }
            "invoice.payment_succeeded" => {
                if let Some(sub_id) = extract_invoice_subscription_id(&obj) {
                    let reason = obj
                        .get("billing_reason")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if matches!(
                        reason,
                        "subscription"
                            | "subscription_create"
                            | "subscription_cycle"
                            | "subscription_update"
                    ) {
                        self.subscription_repo
                            .reset_usage_tx(&mut tx, &sub_id)
                            .await
                            .map_err(|e| BillingError::Other(anyhow!(e)))?;
                    }
                }
            }
            "invoice.payment_failed" => {
                if let Some(sub_id) = extract_invoice_subscription_id(&obj) {
                    self.subscription_repo
                        .mark_past_due_tx(&mut tx, &sub_id)
                        .await
                        .map_err(|e| BillingError::Other(anyhow!(e)))?;
                }
            }
            _ => {}
        }

        sqlx::query("INSERT INTO stripe_webhook_events (event_id, event_type) VALUES ($1, $2)")
            .bind(&event_id)
            .bind(&event_type)
            .execute(&mut *tx)
            .await
            .map_err(|e| BillingError::Other(anyhow!(e)))?;

        tx.commit()
            .await
            .map_err(|e| BillingError::Other(anyhow!(e)))?;
        Ok(())
    }

    async fn handle_checkout_completed_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        obj: &JsonValue,
    ) -> Result<(), BillingError> {
        let user_id = match extract_user_id_from_checkout(obj) {
            Some(u) => u,
            None => return Ok(()),
        };

        let customer_id = obj
            .get("customer")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if !customer_id.is_empty() {
            self.subscription_repo
                .set_stripe_customer_id_tx(tx, user_id, &customer_id)
                .await
                .map_err(|e| BillingError::Other(anyhow!(e)))?;
        }

        let kind = obj
            .get("metadata")
            .and_then(|m| m.get("purchase_kind"))
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if kind == "extra_cinematic" {
            let paid = obj.get("payment_status").and_then(|v| v.as_str()) == Some("paid");
            if paid {
                self.subscription_repo
                    .add_extra_cinematic_credits_tx(tx, user_id, 1)
                    .await
                    .map_err(|e| BillingError::Other(anyhow!(e)))?;
            }
        }
        Ok(())
    }

    async fn handle_subscription_updated_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        obj: &JsonValue,
    ) -> Result<(), BillingError> {
        let sub_id = obj.get("id").and_then(|v| v.as_str()).unwrap_or_default();
        if sub_id.is_empty() {
            return Ok(());
        }
        let stripe_status = obj.get("status").and_then(|v| v.as_str()).unwrap_or("");
        let mapped = map_stripe_status(stripe_status);
        if mapped == subscription_status::CANCELED {
            self.subscription_repo
                .cancel_subscription_tx(tx, sub_id)
                .await
                .map_err(|e| BillingError::Other(anyhow!(e)))?;
            return Ok(());
        }

        let user_id = match self.resolve_subscription_user_id(obj).await {
            Some(u) => u,
            None => return Ok(()),
        };

        let customer_id = obj
            .get("customer")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let price_id = obj
            .get("items")
            .and_then(|i| i.get("data"))
            .and_then(|a| a.as_array())
            .and_then(|arr| arr.get(0))
            .and_then(|item| item.get("price"))
            .and_then(|p| p.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let plan = if price_id == self.starter_price_id {
            plan_type::EXPLORER
        } else if price_id == self.pro_price_id {
            plan_type::PRO
        } else if price_id == self.family_price_id {
            plan_type::UNLIMITED
        } else {
            return Ok(());
        };

        let (period_start, period_end) = stripe_subscription_period(obj);
        let cancel_at_period_end = obj
            .get("cancel_at_period_end")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        self.subscription_repo
            .upsert_from_stripe_tx(
                tx,
                user_id,
                &customer_id,
                sub_id,
                plan,
                mapped,
                period_start,
                period_end,
                cancel_at_period_end,
            )
            .await
            .map_err(|e| BillingError::Other(anyhow!(e)))?;
        Ok(())
    }

    async fn resolve_subscription_user_id(&self, obj: &JsonValue) -> Option<Uuid> {
        if let Some(raw) = obj
            .get("metadata")
            .and_then(|m| m.get("user_id"))
            .and_then(|v| v.as_str())
        {
            if let Ok(u) = Uuid::parse_str(raw) {
                return Some(u);
            }
        }
        if let Some(cust) = obj
            .get("customer")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            if let Ok(Some(s)) = self.subscription_repo.get_by_stripe_customer_id(cust).await {
                return Some(s.user_id);
            }
        }
        None
    }

    async fn ensure_stripe_customer(&self, user_id: Uuid) -> Result<String, BillingError> {
        let sub = self
            .subscription_repo
            .ensure_free_subscription(user_id)
            .await
            .map_err(|e| BillingError::Other(anyhow!(e)))?;
        if let Some(id) = sub.stripe_customer_id.as_deref().filter(|s| !s.is_empty()) {
            return Ok(id.to_string());
        }
        let user = self
            .user_repo
            .get_user_by_id(user_id)
            .await
            .map_err(|e| BillingError::Other(anyhow!(e)))?;
        #[derive(Deserialize)]
        struct CustomerResp {
            id: String,
        }
        let r: CustomerResp = self
            .stripe_post(
                "/customers",
                vec![
                    ("email", user.email.clone()),
                    ("metadata[user_id]", user_id.to_string()),
                ],
            )
            .await
            .map_err(BillingError::Other)?;
        self.subscription_repo
            .set_stripe_customer_id(user_id, &r.id)
            .await
            .map_err(|e| BillingError::Other(anyhow!(e)))?;
        Ok(r.id)
    }

    async fn stripe_post<R: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        form: Vec<(&str, String)>,
    ) -> Result<R> {
        let url = format!("{}{}", STRIPE_API, path);
        let resp = self
            .http
            .post(&url)
            .basic_auth(&self.stripe_secret_key, Some(""))
            .form(&form)
            .send()
            .await?;
        let status = resp.status();
        let raw = resp.text().await?;
        if !status.is_success() {
            return Err(anyhow!("stripe {} -> {}: {}", path, status, raw));
        }
        serde_json::from_str::<R>(&raw)
            .map_err(|e| anyhow!("stripe decode {}: {} (raw={})", path, e, raw))
    }
}

fn map_stripe_status(status: &str) -> &'static str {
    match status {
        "active" => subscription_status::ACTIVE,
        "trialing" => subscription_status::TRIALING,
        "canceled" => subscription_status::CANCELED,
        _ => subscription_status::PAST_DUE,
    }
}

fn stripe_subscription_period(obj: &JsonValue) -> (DateTime<Utc>, DateTime<Utc>) {
    let now = Utc::now();
    let mut start = obj
        .get("start_date")
        .and_then(|v| v.as_i64())
        .and_then(|t| Utc.timestamp_opt(t, 0).single())
        .unwrap_or(now);
    let mut end = start;
    if let Some(items) = obj
        .get("items")
        .and_then(|i| i.get("data"))
        .and_then(|a| a.as_array())
    {
        if let Some(item) = items.get(0) {
            if let Some(t) = item.get("current_period_start").and_then(|v| v.as_i64()) {
                if let Some(d) = Utc.timestamp_opt(t, 0).single() {
                    start = d;
                }
            }
            if let Some(t) = item.get("current_period_end").and_then(|v| v.as_i64()) {
                if let Some(d) = Utc.timestamp_opt(t, 0).single() {
                    end = d;
                }
            }
        }
    }
    if end < start {
        end = start;
    }
    (start, end)
}

fn extract_user_id_from_checkout(obj: &JsonValue) -> Option<Uuid> {
    if let Some(raw) = obj
        .get("metadata")
        .and_then(|m| m.get("user_id"))
        .and_then(|v| v.as_str())
    {
        if let Ok(u) = Uuid::parse_str(raw) {
            return Some(u);
        }
    }
    obj.get("client_reference_id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
}

fn extract_invoice_subscription_id(obj: &JsonValue) -> Option<String> {
    // Current field: invoice.subscription (string)
    if let Some(s) = obj.get("subscription").and_then(|v| v.as_str()) {
        if !s.is_empty() {
            return Some(s.to_string());
        }
    }
    // New field (API version 2024-09+): invoice.parent.subscription_details.subscription
    obj.get("parent")
        .and_then(|p| p.get("subscription_details"))
        .and_then(|d| d.get("subscription"))
        .and_then(|s| s.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn verify_stripe_signature(secret: &str, sig_header: &str, payload: &[u8]) -> Result<JsonValue> {
    let mut timestamp: Option<&str> = None;
    let mut signatures: Vec<&str> = vec![];
    for part in sig_header.split(',') {
        if let Some((k, v)) = part.split_once('=') {
            match k {
                "t" => timestamp = Some(v),
                "v1" => signatures.push(v),
                _ => {}
            }
        }
    }
    let ts = timestamp.ok_or_else(|| anyhow!("stripe: missing timestamp"))?;
    if signatures.is_empty() {
        return Err(anyhow!("stripe: missing v1 signature"));
    }
    let signed_payload = format!("{}.{}", ts, std::str::from_utf8(payload)?);
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())?;
    mac.update(signed_payload.as_bytes());
    let expected = hex::encode(mac.finalize().into_bytes());

    let mut matched = false;
    for sig in signatures {
        if sig.as_bytes().ct_eq(expected.as_bytes()).into() {
            matched = true;
            break;
        }
    }
    if !matched {
        return Err(anyhow!("stripe: signature mismatch"));
    }
    let event: JsonValue = serde_json::from_slice(payload)?;
    Ok(event)
}
