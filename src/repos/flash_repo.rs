use crate::models::*;
use anyhow::Result;
use rand::RngCore;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(FromRow)]
struct FlashVisionListRow {
    #[sqlx(flatten)]
    vision: FlashVision,
    cover_image_url: Option<String>,
}

#[derive(Clone)]
pub struct FlashRepository {
    pool: PgPool,
}

impl FlashRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn create_flash_vision(&self, v: &FlashVision) -> Result<()> {
        sqlx::query(
            "INSERT INTO flash_visions
               (id, user_id, question, input_method, status, photo_url, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, now(), now())",
        )
        .bind(v.id)
        .bind(v.user_id)
        .bind(&v.question)
        .bind(&v.input_method)
        .bind(&v.status)
        .bind(&v.photo_url)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_flash_vision_by_id(&self, id: Uuid) -> Result<FlashVision> {
        let v = sqlx::query_as::<_, FlashVision>(
            "SELECT id, user_id, question, input_method, status, photo_url, music_url,
                    error_message, share_token, is_public, completed_at, created_at, updated_at
             FROM flash_visions WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await?;
        Ok(v)
    }

    pub async fn get_flash_vision_by_share_token(&self, token: &str) -> Result<FlashVision> {
        let v = sqlx::query_as::<_, FlashVision>(
            "SELECT id, user_id, question, input_method, status, photo_url, music_url,
                    error_message, share_token, is_public, completed_at, created_at, updated_at
             FROM flash_visions WHERE share_token = $1 AND is_public = true",
        )
        .bind(token)
        .fetch_one(&self.pool)
        .await?;
        Ok(v)
    }

    pub async fn update_flash_vision_status(
        &self,
        id: Uuid,
        status: &str,
        error_msg: Option<&str>,
    ) -> Result<()> {
        let q = if status == flash_status::COMPLETED {
            "UPDATE flash_visions SET status = $1, error_message = $2, completed_at = now(), updated_at = now() WHERE id = $3"
        } else {
            "UPDATE flash_visions SET status = $1, error_message = $2, updated_at = now() WHERE id = $3"
        };
        sqlx::query(q).bind(status).bind(error_msg).bind(id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn set_music_url(&self, id: Uuid, music_url: &str) -> Result<()> {
        sqlx::query("UPDATE flash_visions SET music_url = $1, updated_at = now() WHERE id = $2")
            .bind(music_url)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn list_flash_visions_by_user(
        &self,
        user_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<(FlashVision, Option<String>)>, i64)> {
        let total_row: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM flash_visions WHERE user_id = $1")
                .bind(user_id)
                .fetch_one(&self.pool)
                .await?;
        let rows = sqlx::query_as::<_, FlashVisionListRow>(
            r#"SELECT v.id, v.user_id, v.question, v.input_method, v.status, v.photo_url,
                      v.music_url, v.error_message, v.share_token, v.is_public,
                      v.completed_at, v.created_at, v.updated_at,
                      i.storage_url AS cover_image_url
               FROM flash_visions v
               LEFT JOIN LATERAL (
                 SELECT storage_url
                 FROM flash_images
                 WHERE flash_vision_id = v.id
                 ORDER BY "index" ASC
                 LIMIT 1
               ) i ON true
               WHERE v.user_id = $1
               ORDER BY v.created_at DESC
               LIMIT $2 OFFSET $3"#,
        )
        .bind(user_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        let items = rows
            .into_iter()
            .map(|r| (r.vision, r.cover_image_url))
            .collect();
        Ok((items, total_row.0))
    }

    pub async fn create_flash_image(&self, img: &FlashImage) -> Result<()> {
        sqlx::query(
            r#"INSERT INTO flash_images
               (id, flash_vision_id, "index", storage_url, storage_path, prompt_used, style_reference_id, generation_metadata, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, now())"#,
        )
        .bind(img.id)
        .bind(img.flash_vision_id)
        .bind(img.index)
        .bind(&img.storage_url)
        .bind(&img.storage_path)
        .bind(&img.prompt_used)
        .bind(img.style_reference_id)
        .bind(&img.generation_metadata)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_flash_images_by_vision_id(&self, vision_id: Uuid) -> Result<Vec<FlashImage>> {
        let rows = sqlx::query_as::<_, FlashImage>(
            r#"SELECT id, flash_vision_id, "index", storage_url, storage_path, prompt_used,
                    style_reference_id, COALESCE(generation_metadata, '{}'::jsonb) AS generation_metadata, created_at
             FROM flash_images WHERE flash_vision_id = $1 ORDER BY "index""#,
        )
        .bind(vision_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn count_completed_images(&self, vision_id: Uuid) -> Result<i64> {
        let row: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM flash_images WHERE flash_vision_id = $1")
                .bind(vision_id)
                .fetch_one(&self.pool)
                .await?;
        Ok(row.0)
    }

    pub async fn set_share_token(&self, id: Uuid) -> Result<String> {
        let mut bytes = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut bytes);
        let token = hex::encode(bytes);
        sqlx::query(
            "UPDATE flash_visions SET share_token = $1, is_public = true, updated_at = now() WHERE id = $2",
        )
        .bind(&token)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(token)
    }
}
