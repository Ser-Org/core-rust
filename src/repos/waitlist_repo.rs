use crate::models::*;
use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Clone)]
pub struct WaitlistRepository {
    pool: PgPool,
}

impl WaitlistRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn insert(
        &self,
        email: &str,
        name: Option<&str>,
        ip: Option<&str>,
        ua: Option<&str>,
        source: &str,
    ) -> Result<Uuid> {
        let row: (Uuid,) = sqlx::query_as(
            "INSERT INTO waitlist_entries (id, email, name, ip_address, user_agent, source, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, now())
             ON CONFLICT (email) DO UPDATE SET source = EXCLUDED.source
             RETURNING id",
        )
        .bind(Uuid::new_v4())
        .bind(email)
        .bind(name)
        .bind(ip)
        .bind(ua)
        .bind(source)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    pub async fn list(&self) -> Result<Vec<WaitlistEntry>> {
        let rows = sqlx::query_as::<_, WaitlistEntry>(
            "SELECT id, email, name, ip_address, user_agent, source, created_at FROM waitlist_entries ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}
