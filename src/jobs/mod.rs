//! Simple Postgres-backed job queue.
//!
//! Schema (created by migration 100):
//!   scout_jobs(id UUID, kind TEXT, args JSONB, state TEXT,
//!              scheduled_at TIMESTAMPTZ, attempts INT, max_attempts INT,
//!              last_error TEXT, created_at, updated_at)
//!
//! Workers poll the table, claim rows with SELECT ... FOR UPDATE SKIP LOCKED,
//! run the handler, and mark the row "completed" or "failed".

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

pub mod worker;

// Job kinds (must stay stable).
pub const KIND_LIFE_STATE_EXTRACTION: &str = "life_state_extraction";
pub const KIND_SCENARIO_PLANNER: &str = "scenario_planner";
pub const KIND_ASSUMPTION_EXTRACTION: &str = "assumption_extraction";
pub const KIND_CHARACTER_PLATE: &str = "character_plate";
pub const KIND_VIDEO_GENERATION: &str = "video_generation";
pub const KIND_FLASH_GENERATION: &str = "flash_generation";

#[derive(Clone)]
pub struct JobClient {
    pool: PgPool,
}

impl JobClient {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn insert<T: Serialize>(&self, kind: &str, args: &T) -> Result<Uuid> {
        let args_json = serde_json::to_value(args)?;
        let row: (Uuid,) = sqlx::query_as(
            "INSERT INTO scout_jobs (id, kind, args, state, scheduled_at, attempts, max_attempts, created_at, updated_at)
             VALUES ($1, $2, $3, 'pending', now(), 0, 3, now(), now())
             RETURNING id",
        )
        .bind(Uuid::new_v4())
        .bind(kind)
        .bind(args_json)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    pub async fn insert_with_opts<T: Serialize>(
        &self,
        kind: &str,
        args: &T,
        max_attempts: i32,
    ) -> Result<Uuid> {
        let args_json = serde_json::to_value(args)?;
        let row: (Uuid,) = sqlx::query_as(
            "INSERT INTO scout_jobs (id, kind, args, state, scheduled_at, attempts, max_attempts, created_at, updated_at)
             VALUES ($1, $2, $3, 'pending', now(), 0, $4, now(), now())
             RETURNING id",
        )
        .bind(Uuid::new_v4())
        .bind(kind)
        .bind(args_json)
        .bind(max_attempts)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    pub async fn claim_one(&self) -> Result<Option<JobRow>> {
        let row = sqlx::query_as::<_, JobRow>(
            "UPDATE scout_jobs
             SET state = 'running', attempts = attempts + 1, updated_at = now()
             WHERE id = (
                SELECT id FROM scout_jobs
                WHERE state = 'pending' AND scheduled_at <= now()
                ORDER BY scheduled_at
                FOR UPDATE SKIP LOCKED
                LIMIT 1
             )
             RETURNING id, kind, args, state, scheduled_at, attempts, max_attempts, last_error, created_at, updated_at",
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn mark_completed(&self, id: Uuid) -> Result<()> {
        sqlx::query("UPDATE scout_jobs SET state = 'completed', updated_at = now() WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn mark_failed(&self, id: Uuid, err: &str, retryable: bool) -> Result<()> {
        if retryable {
            sqlx::query(
                "UPDATE scout_jobs
                 SET state = CASE WHEN attempts >= max_attempts THEN 'failed' ELSE 'pending' END,
                     scheduled_at = now() + ((attempts * 30) || ' seconds')::interval,
                     last_error = $2,
                     updated_at = now()
                 WHERE id = $1",
            )
            .bind(id)
            .bind(err)
            .execute(&self.pool)
            .await?;
        } else {
            sqlx::query(
                "UPDATE scout_jobs SET state = 'failed', last_error = $2, updated_at = now() WHERE id = $1",
            )
            .bind(id)
            .bind(err)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct JobRow {
    pub id: Uuid,
    pub kind: String,
    pub args: JsonValue,
    pub state: String,
    pub scheduled_at: DateTime<Utc>,
    pub attempts: i32,
    pub max_attempts: i32,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// Args structs --------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifeStateExtractionArgs {
    pub user_id: Uuid,
    pub story_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioPlannerArgs {
    pub simulation_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssumptionExtractionArgs {
    pub simulation_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterPlateArgs {
    pub user_id: Uuid,
    pub source_photo_id: Uuid,
    pub plate_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoGenerationArgs {
    pub simulation_id: Uuid,
    pub path: String,
    pub phase: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashGenerationArgs {
    pub flash_vision_id: Uuid,
    pub user_id: Uuid,
}
