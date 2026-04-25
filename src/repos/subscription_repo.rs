use crate::models::*;
use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Clone)]
pub struct SubscriptionRepository {
    pool: PgPool,
}

const COLUMNS: &str = "id, user_id, stripe_customer_id, stripe_subscription_id,
    plan, status, cinematic_used, cinematic_limit,
    text_resim_used, text_resim_limit, extra_cinematic_credits,
    flash_used, flash_limit,
    period_start, period_end,
    cancel_at_period_end, created_at, updated_at";

impl SubscriptionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn get_by_user_id(&self, user_id: Uuid) -> Result<Option<Subscription>> {
        let sub = sqlx::query_as::<_, Subscription>(&format!(
            "SELECT {} FROM subscriptions WHERE user_id = $1",
            COLUMNS
        ))
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(sub)
    }

    pub async fn get_by_stripe_customer_id(&self, cust: &str) -> Result<Option<Subscription>> {
        let sub = sqlx::query_as::<_, Subscription>(&format!(
            "SELECT {} FROM subscriptions WHERE stripe_customer_id = $1",
            COLUMNS
        ))
        .bind(cust)
        .fetch_optional(&self.pool)
        .await?;
        Ok(sub)
    }

    pub async fn get_by_stripe_subscription_id(&self, sid: &str) -> Result<Option<Subscription>> {
        let sub = sqlx::query_as::<_, Subscription>(&format!(
            "SELECT {} FROM subscriptions WHERE stripe_subscription_id = $1",
            COLUMNS
        ))
        .bind(sid)
        .fetch_optional(&self.pool)
        .await?;
        Ok(sub)
    }

    pub async fn ensure_free_subscription(&self, user_id: Uuid) -> Result<Subscription> {
        let cinematic = plan_type::simulation_limit(plan_type::FREE);
        let flash = plan_type::flash_limit(plan_type::FREE);
        let sub = sqlx::query_as::<_, Subscription>(&format!(
            "INSERT INTO subscriptions
                (id, user_id, plan, status, cinematic_used, cinematic_limit,
                 text_resim_used, text_resim_limit, extra_cinematic_credits,
                 flash_used, flash_limit)
             VALUES ($1, $2, $3, 'active', 0, $4, 0, 0, 0, 0, $5)
             ON CONFLICT (user_id) DO UPDATE SET updated_at = now()
             RETURNING {}",
            COLUMNS
        ))
        .bind(Uuid::new_v4())
        .bind(user_id)
        .bind(plan_type::FREE)
        .bind(cinematic)
        .bind(flash)
        .fetch_one(&self.pool)
        .await?;
        Ok(sub)
    }

    pub async fn ensure_free_subscription_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        user_id: Uuid,
    ) -> Result<Subscription> {
        let cinematic = plan_type::simulation_limit(plan_type::FREE);
        let flash = plan_type::flash_limit(plan_type::FREE);
        let sub = sqlx::query_as::<_, Subscription>(&format!(
            "INSERT INTO subscriptions
                (id, user_id, plan, status, cinematic_used, cinematic_limit,
                 text_resim_used, text_resim_limit, extra_cinematic_credits,
                 flash_used, flash_limit)
             VALUES ($1, $2, $3, 'active', 0, $4, 0, 0, 0, 0, $5)
             ON CONFLICT (user_id) DO UPDATE SET updated_at = now()
             RETURNING {}",
            COLUMNS
        ))
        .bind(Uuid::new_v4())
        .bind(user_id)
        .bind(plan_type::FREE)
        .bind(cinematic)
        .bind(flash)
        .fetch_one(&mut **tx)
        .await?;
        Ok(sub)
    }

    pub async fn upsert_from_stripe(
        &self,
        user_id: Uuid,
        stripe_customer_id: &str,
        stripe_subscription_id: &str,
        plan: &str,
        status: &str,
        period_start: DateTime<Utc>,
        period_end: DateTime<Utc>,
        cancel_at_period_end: bool,
    ) -> Result<Subscription> {
        let mut tx = self.pool.begin().await?;
        let sub = self
            .upsert_from_stripe_tx(
                &mut tx,
                user_id,
                stripe_customer_id,
                stripe_subscription_id,
                plan,
                status,
                period_start,
                period_end,
                cancel_at_period_end,
            )
            .await?;
        tx.commit().await?;
        Ok(sub)
    }

    pub async fn upsert_from_stripe_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        user_id: Uuid,
        stripe_customer_id: &str,
        stripe_subscription_id: &str,
        plan: &str,
        status: &str,
        period_start: DateTime<Utc>,
        period_end: DateTime<Utc>,
        cancel_at_period_end: bool,
    ) -> Result<Subscription> {
        let cin_limit = plan_type::simulation_limit(plan);
        let flash_limit = plan_type::flash_limit(plan);
        let sub = sqlx::query_as::<_, Subscription>(&format!(
            "INSERT INTO subscriptions
                (id, user_id, stripe_customer_id, stripe_subscription_id,
                 plan, status, cinematic_used, cinematic_limit,
                 text_resim_used, text_resim_limit, extra_cinematic_credits,
                 flash_used, flash_limit,
                 period_start, period_end, cancel_at_period_end)
             VALUES ($1, $2, $3, $4, $5, $6, 0, $7, 0, 0, 0, 0, $8, $9, $10, $11)
             ON CONFLICT (user_id) DO UPDATE SET
                stripe_customer_id     = EXCLUDED.stripe_customer_id,
                stripe_subscription_id = EXCLUDED.stripe_subscription_id,
                plan                   = EXCLUDED.plan,
                status                 = EXCLUDED.status,
                cinematic_limit        = EXCLUDED.cinematic_limit,
                flash_limit            = EXCLUDED.flash_limit,
                period_start           = EXCLUDED.period_start,
                period_end             = EXCLUDED.period_end,
                cancel_at_period_end   = EXCLUDED.cancel_at_period_end,
                updated_at             = now()
             RETURNING {}",
            COLUMNS
        ))
        .bind(Uuid::new_v4())
        .bind(user_id)
        .bind(stripe_customer_id)
        .bind(stripe_subscription_id)
        .bind(plan)
        .bind(status)
        .bind(cin_limit)
        .bind(flash_limit)
        .bind(period_start)
        .bind(period_end)
        .bind(cancel_at_period_end)
        .fetch_one(&mut **tx)
        .await?;
        Ok(sub)
    }

    pub async fn cancel_subscription(&self, stripe_subscription_id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.cancel_subscription_tx(&mut tx, stripe_subscription_id)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn cancel_subscription_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        stripe_subscription_id: &str,
    ) -> Result<()> {
        let cin = plan_type::simulation_limit(plan_type::FREE);
        let flash = plan_type::flash_limit(plan_type::FREE);
        sqlx::query(
            "UPDATE subscriptions
             SET status = 'canceled', plan = 'free',
                 cinematic_limit = $2, text_resim_limit = 0, flash_limit = $3,
                 updated_at = now()
             WHERE stripe_subscription_id = $1",
        )
        .bind(stripe_subscription_id)
        .bind(cin)
        .bind(flash)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    pub async fn reset_usage(&self, stripe_subscription_id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.reset_usage_tx(&mut tx, stripe_subscription_id).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn reset_usage_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        stripe_subscription_id: &str,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE subscriptions
             SET cinematic_used = 0, text_resim_used = 0, flash_used = 0, updated_at = now()
             WHERE stripe_subscription_id = $1",
        )
        .bind(stripe_subscription_id)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    pub async fn set_stripe_customer_id(&self, user_id: Uuid, customer_id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.set_stripe_customer_id_tx(&mut tx, user_id, customer_id)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn set_stripe_customer_id_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        user_id: Uuid,
        customer_id: &str,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE subscriptions SET stripe_customer_id = $1, updated_at = now() WHERE user_id = $2",
        )
        .bind(customer_id)
        .bind(user_id)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    pub async fn mark_past_due_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        stripe_subscription_id: &str,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE subscriptions SET status = 'past_due', updated_at = now() WHERE stripe_subscription_id = $1",
        )
        .bind(stripe_subscription_id)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    pub async fn mark_past_due(&self, stripe_subscription_id: &str) -> Result<()> {
        sqlx::query(
            "UPDATE subscriptions SET status = 'past_due', updated_at = now() WHERE stripe_subscription_id = $1",
        )
        .bind(stripe_subscription_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn add_extra_cinematic_credits(
        &self,
        user_id: Uuid,
        credits: i32,
    ) -> Result<Subscription> {
        let mut tx = self.pool.begin().await?;
        let sub = self
            .add_extra_cinematic_credits_tx(&mut tx, user_id, credits)
            .await?;
        tx.commit().await?;
        Ok(sub)
    }

    pub async fn add_extra_cinematic_credits_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        user_id: Uuid,
        credits: i32,
    ) -> Result<Subscription> {
        if credits <= 0 {
            return self.ensure_free_subscription_tx(tx, user_id).await;
        }
        self.ensure_free_subscription_tx(tx, user_id).await?;
        let sub = sqlx::query_as::<_, Subscription>(&format!(
            "UPDATE subscriptions
             SET extra_cinematic_credits = extra_cinematic_credits + $1, updated_at = now()
             WHERE user_id = $2
             RETURNING {}",
            COLUMNS
        ))
        .bind(credits)
        .bind(user_id)
        .fetch_one(&mut **tx)
        .await?;
        Ok(sub)
    }

    /// Consume a cinematic entitlement inside a transaction.
    /// Returns `(sub, used_extra_credit)` on success, or an `EntitlementError` via Err.
    pub async fn consume_cinematic_entitlement_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        user_id: Uuid,
    ) -> std::result::Result<(Subscription, bool), EntitlementError> {
        self.ensure_free_subscription_tx(tx, user_id)
            .await
            .map_err(|e| EntitlementError {
                code: "internal".into(),
                message: e.to_string(),
            })?;
        let sub = sqlx::query_as::<_, Subscription>(&format!(
            "SELECT {} FROM subscriptions WHERE user_id = $1 FOR UPDATE",
            COLUMNS
        ))
        .bind(user_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| EntitlementError {
            code: "internal".into(),
            message: e.to_string(),
        })?;

        if sub.billing_active() && sub.cinematic_used < sub.cinematic_limit {
            let updated = sqlx::query_as::<_, Subscription>(&format!(
                "UPDATE subscriptions SET cinematic_used = cinematic_used + 1, updated_at = now() WHERE user_id = $1 RETURNING {}",
                COLUMNS
            ))
            .bind(user_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|e| EntitlementError {
                code: "internal".into(),
                message: e.to_string(),
            })?;
            return Ok((updated, false));
        }
        if sub.extra_cinematic_credits > 0 {
            let updated = sqlx::query_as::<_, Subscription>(&format!(
                "UPDATE subscriptions SET extra_cinematic_credits = extra_cinematic_credits - 1, updated_at = now() WHERE user_id = $1 RETURNING {}",
                COLUMNS
            ))
            .bind(user_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|e| EntitlementError {
                code: "internal".into(),
                message: e.to_string(),
            })?;
            return Ok((updated, true));
        }
        if sub.plan == plan_type::FREE {
            return Err(EntitlementError {
                code: entitlement_code::CINEMATIC_LIMIT_REACHED.into(),
                message: "upgrade to run a cinematic simulation".into(),
            });
        }
        if !sub.billing_active() {
            return Err(EntitlementError {
                code: entitlement_code::BILLING_INACTIVE.into(),
                message: "your billing status is inactive".into(),
            });
        }
        Err(EntitlementError {
            code: entitlement_code::CINEMATIC_LIMIT_REACHED.into(),
            message: "your cinematic simulation quota has been used".into(),
        })
    }

    pub async fn check_cinematic_entitlement(
        &self,
        user_id: Uuid,
    ) -> std::result::Result<(), EntitlementError> {
        let sub = self
            .ensure_free_subscription(user_id)
            .await
            .map_err(|e| EntitlementError {
                code: "internal".into(),
                message: e.to_string(),
            })?;

        if sub.billing_active() && sub.cinematic_used < sub.cinematic_limit {
            return Ok(());
        }
        if sub.extra_cinematic_credits > 0 {
            return Ok(());
        }
        if sub.plan == plan_type::FREE {
            return Err(EntitlementError {
                code: entitlement_code::CINEMATIC_LIMIT_REACHED.into(),
                message: "upgrade to run a cinematic simulation".into(),
            });
        }
        if !sub.billing_active() {
            return Err(EntitlementError {
                code: entitlement_code::BILLING_INACTIVE.into(),
                message: "your billing status is inactive".into(),
            });
        }
        Err(EntitlementError {
            code: entitlement_code::CINEMATIC_LIMIT_REACHED.into(),
            message: "your cinematic simulation quota has been used".into(),
        })
    }

    pub async fn consume_flash_entitlement_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        user_id: Uuid,
    ) -> std::result::Result<Subscription, EntitlementError> {
        self.ensure_free_subscription_tx(tx, user_id)
            .await
            .map_err(|e| EntitlementError {
                code: "internal".into(),
                message: e.to_string(),
            })?;
        let sub = sqlx::query_as::<_, Subscription>(&format!(
            "SELECT {} FROM subscriptions WHERE user_id = $1 FOR UPDATE",
            COLUMNS
        ))
        .bind(user_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| EntitlementError {
            code: "internal".into(),
            message: e.to_string(),
        })?;

        if sub.billing_active() && sub.flash_used < sub.flash_limit {
            let updated = sqlx::query_as::<_, Subscription>(&format!(
                "UPDATE subscriptions SET flash_used = flash_used + 1, updated_at = now() WHERE user_id = $1 RETURNING {}",
                COLUMNS
            ))
            .bind(user_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|e| EntitlementError {
                code: "internal".into(),
                message: e.to_string(),
            })?;
            return Ok(updated);
        }
        if sub.plan == plan_type::FREE {
            return Err(EntitlementError {
                code: entitlement_code::FLASH_LIMIT_REACHED.into(),
                message: "upgrade to create more Flash visions".into(),
            });
        }
        if !sub.billing_active() {
            return Err(EntitlementError {
                code: entitlement_code::BILLING_INACTIVE.into(),
                message: "your billing status is inactive".into(),
            });
        }
        Err(EntitlementError {
            code: entitlement_code::FLASH_LIMIT_REACHED.into(),
            message: "your Flash vision quota has been used".into(),
        })
    }

    pub async fn check_flash_entitlement(
        &self,
        user_id: Uuid,
    ) -> std::result::Result<(), EntitlementError> {
        let sub = self
            .ensure_free_subscription(user_id)
            .await
            .map_err(|e| EntitlementError {
                code: "internal".into(),
                message: e.to_string(),
            })?;

        if sub.billing_active() && sub.flash_used < sub.flash_limit {
            return Ok(());
        }
        if sub.plan == plan_type::FREE {
            return Err(EntitlementError {
                code: entitlement_code::FLASH_LIMIT_REACHED.into(),
                message: "upgrade to create more Flash visions".into(),
            });
        }
        if !sub.billing_active() {
            return Err(EntitlementError {
                code: entitlement_code::BILLING_INACTIVE.into(),
                message: "your billing status is inactive".into(),
            });
        }
        Err(EntitlementError {
            code: entitlement_code::FLASH_LIMIT_REACHED.into(),
            message: "your Flash vision quota has been used".into(),
        })
    }
}
