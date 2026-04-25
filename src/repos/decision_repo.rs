use crate::models::*;
use anyhow::{anyhow, Result};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Clone)]
pub struct DecisionRepository {
    pool: PgPool,
}

impl DecisionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn create_decision(&self, d: &Decision) -> Result<()> {
        sqlx::query(
            "INSERT INTO decisions (id, user_id, decision_text, input_method, time_horizon_months, status, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, now(), now())",
        )
        .bind(d.id)
        .bind(d.user_id)
        .bind(&d.decision_text)
        .bind(&d.input_method)
        .bind(d.time_horizon_months)
        .bind(&d.status)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_decision_by_id(&self, id: Uuid) -> Result<Decision> {
        let d = sqlx::query_as::<_, Decision>(
            "SELECT id, user_id, decision_text, input_method, time_horizon_months, status, category, severity, reversibility, share_token, created_at, updated_at
             FROM decisions WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await?;
        Ok(d)
    }

    pub async fn list_decisions_by_user_id(&self, user_id: Uuid) -> Result<Vec<Decision>> {
        let rows = sqlx::query_as::<_, Decision>(
            "SELECT id, user_id, decision_text, input_method, time_horizon_months, status, category, severity, reversibility, share_token, created_at, updated_at
             FROM decisions WHERE user_id = $1
             ORDER BY created_at DESC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn update_decision_status(&self, id: Uuid, status: &str) -> Result<()> {
        let res = sqlx::query("UPDATE decisions SET status = $1, updated_at = now() WHERE id = $2")
            .bind(status)
            .bind(id)
            .execute(&self.pool)
            .await?;
        if res.rows_affected() == 0 {
            return Err(anyhow!("decision {} not found", id));
        }
        Ok(())
    }

    pub async fn update_time_horizon_months(&self, id: Uuid, months: i32) -> Result<()> {
        sqlx::query(
            "UPDATE decisions SET time_horizon_months = $1, updated_at = now() WHERE id = $2",
        )
        .bind(months)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
