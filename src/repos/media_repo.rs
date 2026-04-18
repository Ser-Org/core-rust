use crate::models::*;
use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Clone)]
pub struct MediaRepository {
    pool: PgPool,
}

impl MediaRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn insert_generated_media(&self, m: &GeneratedMedia) -> Result<()> {
        sqlx::query(
            "INSERT INTO generated_media
               (id, simulation_id, media_type, storage_url, storage_path, prompt_used, provider_metadata, clip_role, clip_order, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, now())",
        )
        .bind(m.id)
        .bind(m.simulation_id)
        .bind(&m.media_type)
        .bind(&m.storage_url)
        .bind(&m.storage_path)
        .bind(&m.prompt_used)
        .bind(&m.provider_metadata)
        .bind(&m.clip_role)
        .bind(m.clip_order)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_media_by_simulation_id(&self, sim_id: Uuid) -> Result<Vec<GeneratedMedia>> {
        let rows = sqlx::query_as::<_, GeneratedMedia>(
            "SELECT id, simulation_id, media_type, storage_url, storage_path, prompt_used,
                    COALESCE(provider_metadata, '{}'::jsonb) AS provider_metadata, clip_role, clip_order,
                    scenario_path, scenario_phase, created_at
             FROM generated_media WHERE simulation_id = $1
             ORDER BY created_at",
        )
        .bind(sim_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn get_media_by_simulation_and_type(
        &self,
        sim_id: Uuid,
        media_type: &str,
    ) -> Result<Vec<GeneratedMedia>> {
        let rows = sqlx::query_as::<_, GeneratedMedia>(
            "SELECT id, simulation_id, media_type, storage_url, storage_path, prompt_used,
                    COALESCE(provider_metadata, '{}'::jsonb) AS provider_metadata, clip_role, clip_order,
                    scenario_path, scenario_phase, created_at
             FROM generated_media WHERE simulation_id = $1 AND media_type = $2
             ORDER BY clip_order, created_at",
        )
        .bind(sim_id)
        .bind(media_type)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn get_media_by_id(&self, id: Uuid) -> Result<GeneratedMedia> {
        let m = sqlx::query_as::<_, GeneratedMedia>(
            "SELECT id, simulation_id, media_type, storage_url, storage_path, prompt_used,
                    COALESCE(provider_metadata, '{}'::jsonb) AS provider_metadata, clip_role, clip_order,
                    scenario_path, scenario_phase, created_at
             FROM generated_media WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await?;
        Ok(m)
    }

    pub async fn update_media_scenario_fields(
        &self,
        sim_id: Uuid,
        clip_role: &str,
        path: Option<&str>,
        phase: Option<i32>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE generated_media
             SET scenario_path = $1, scenario_phase = $2
             WHERE simulation_id = $3 AND clip_role = $4",
        )
        .bind(path)
        .bind(phase)
        .bind(sim_id)
        .bind(clip_role)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_media_by_simulation_and_scenario(
        &self,
        sim_id: Uuid,
    ) -> Result<Vec<GeneratedMedia>> {
        let rows = sqlx::query_as::<_, GeneratedMedia>(
            "SELECT id, simulation_id, media_type, storage_url, storage_path, prompt_used,
                    COALESCE(provider_metadata, '{}'::jsonb) AS provider_metadata, clip_role, clip_order,
                    scenario_path, scenario_phase, created_at
             FROM generated_media
             WHERE simulation_id = $1 AND scenario_path IS NOT NULL
             ORDER BY scenario_path, scenario_phase",
        )
        .bind(sim_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn get_video_clips_by_simulation_id(
        &self,
        sim_id: Uuid,
    ) -> Result<Vec<GeneratedMedia>> {
        self.get_media_by_simulation_and_type(sim_id, media_type::VIDEO).await
    }
}
