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

    pub async fn update_category(&self, id: Uuid, category: &str) -> Result<()> {
        sqlx::query("UPDATE decisions SET category = $1, updated_at = now() WHERE id = $2")
            .bind(category)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_severity_and_reversibility(
        &self,
        id: Uuid,
        severity: i32,
        reversibility: &str,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE decisions SET severity = $1, reversibility = $2, updated_at = now() WHERE id = $3",
        )
        .bind(severity)
        .bind(reversibility)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
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
        sqlx::query("UPDATE decisions SET time_horizon_months = $1, updated_at = now() WHERE id = $2")
            .bind(months)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn bulk_insert_clarifying_questions(
        &self,
        questions: &[ClarifyingQuestion],
    ) -> Result<()> {
        if questions.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        for q in questions {
            sqlx::query(
                "INSERT INTO clarifying_questions (id, decision_id, question_text, answer_text, answer_method, sort_order, created_at)
                 VALUES ($1, $2, $3, $4, $5, $6, now())",
            )
            .bind(q.id)
            .bind(q.decision_id)
            .bind(&q.question_text)
            .bind(&q.answer_text)
            .bind(&q.answer_method)
            .bind(q.sort_order)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn update_clarifying_answer(
        &self,
        question_id: Uuid,
        text: &str,
        method: &str,
    ) -> Result<()> {
        let res = sqlx::query(
            "UPDATE clarifying_questions SET answer_text = $1, answer_method = $2 WHERE id = $3",
        )
        .bind(text)
        .bind(method)
        .bind(question_id)
        .execute(&self.pool)
        .await?;
        if res.rows_affected() == 0 {
            return Err(anyhow!("clarifying question {} not found", question_id));
        }
        Ok(())
    }

    pub async fn get_clarifying_questions_by_decision_id(
        &self,
        id: Uuid,
    ) -> Result<Vec<ClarifyingQuestion>> {
        let rows = sqlx::query_as::<_, ClarifyingQuestion>(
            "SELECT id, decision_id, question_text, answer_text, answer_method, sort_order, created_at
             FROM clarifying_questions
             WHERE decision_id = $1
             ORDER BY sort_order",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}
