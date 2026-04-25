use crate::models::*;
use anyhow::{anyhow, Result};
use serde_json::{json, Value as JsonValue};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Clone)]
pub struct SimulationComponentsRepo {
    pool: PgPool,
}

impl SimulationComponentsRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn upsert_simulation_components(
        &self,
        components: &[SimulationComponent],
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.upsert_simulation_components_tx(&mut tx, components)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn upsert_simulation_components_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        components: &[SimulationComponent],
    ) -> Result<()> {
        for c in components {
            let meta = if c.metadata.is_null() {
                json!({})
            } else {
                c.metadata.clone()
            };
            let id = if c.id.is_nil() { Uuid::new_v4() } else { c.id };
            sqlx::query(
                "INSERT INTO simulation_components
                   (id, simulation_id, component_key, component_type, display_name, status, path, phase, error_code, error_message,
                    metadata, created_at, started_at, completed_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NULL, NULL, $9, now(), NULL, NULL, now())
                 ON CONFLICT (simulation_id, component_key) DO UPDATE SET
                   component_type = EXCLUDED.component_type,
                   display_name = EXCLUDED.display_name,
                   path = EXCLUDED.path,
                   phase = EXCLUDED.phase,
                   metadata = EXCLUDED.metadata,
                   updated_at = now()",
            )
            .bind(id)
            .bind(c.simulation_id)
            .bind(&c.component_key)
            .bind(&c.component_type)
            .bind(&c.display_name)
            .bind(&c.status)
            .bind(&c.path)
            .bind(c.phase)
            .bind(&meta)
            .execute(&mut **tx)
            .await?;
        }
        Ok(())
    }

    pub async fn mark_component_running(
        &self,
        sim_id: Uuid,
        component_key: &str,
    ) -> Result<String> {
        self.set_component_status(sim_id, component_key, "running", None, None)
            .await
    }

    pub async fn mark_component_completed(
        &self,
        sim_id: Uuid,
        component_key: &str,
    ) -> Result<String> {
        self.set_component_status(sim_id, component_key, "completed", None, None)
            .await
    }

    pub async fn mark_component_failed(
        &self,
        sim_id: Uuid,
        component_key: &str,
        error_code: &str,
        error_message: &str,
    ) -> Result<String> {
        let ec = if error_code.is_empty() {
            None
        } else {
            Some(error_code)
        };
        let em = if error_message.is_empty() {
            None
        } else {
            Some(error_message)
        };
        self.set_component_status(sim_id, component_key, "failed", ec, em)
            .await
    }

    pub async fn mark_components_failed_by_type(
        &self,
        sim_id: Uuid,
        component_type: &str,
        error_code: &str,
        error_message: &str,
    ) -> Result<String> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "UPDATE simulation_components
             SET status = 'failed',
                 error_code = $3,
                 error_message = $4,
                 completed_at = now(),
                 updated_at = now()
             WHERE simulation_id = $1
               AND component_type = $2
               AND status IN ('pending', 'running')",
        )
        .bind(sim_id)
        .bind(component_type)
        .bind(if error_code.is_empty() {
            None
        } else {
            Some(error_code)
        })
        .bind(if error_message.is_empty() {
            None
        } else {
            Some(error_message)
        })
        .execute(&mut *tx)
        .await?;

        let status = self.recompute_simulation_aggregate(&mut tx, sim_id).await?;
        tx.commit().await?;
        Ok(status)
    }

    async fn set_component_status(
        &self,
        sim_id: Uuid,
        component_key: &str,
        status: &str,
        error_code: Option<&str>,
        error_message: Option<&str>,
    ) -> Result<String> {
        let mut tx = self.pool.begin().await?;
        let res = sqlx::query(
            "UPDATE simulation_components
             SET status = $3,
                 error_code = $4,
                 error_message = $5,
                 started_at = CASE WHEN $3 = 'running' THEN COALESCE(started_at, now()) ELSE started_at END,
                 completed_at = CASE WHEN $3 IN ('completed', 'failed') THEN now() ELSE NULL END,
                 updated_at = now()
             WHERE simulation_id = $1 AND component_key = $2",
        )
        .bind(sim_id)
        .bind(component_key)
        .bind(status)
        .bind(error_code)
        .bind(error_message)
        .execute(&mut *tx)
        .await?;
        if res.rows_affected() == 0 {
            return Err(anyhow!(
                "component {} for simulation {} not found",
                component_key,
                sim_id
            ));
        }
        let agg = self.recompute_simulation_aggregate(&mut tx, sim_id).await?;
        tx.commit().await?;
        Ok(agg)
    }

    async fn recompute_simulation_aggregate(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        sim_id: Uuid,
    ) -> Result<String> {
        let row: (String,) = sqlx::query_as(
            "WITH summary AS (
                SELECT
                    COUNT(*) AS total,
                    COUNT(*) FILTER (WHERE status = 'completed') AS completed,
                    COUNT(*) FILTER (WHERE status = 'failed') AS failed,
                    COUNT(*) FILTER (WHERE status IN ('completed', 'failed')) AS resolved
                FROM simulation_components
                WHERE simulation_id = $1
            ), sim_update AS (
                UPDATE decision_simulations AS ds
                SET total_components = summary.total,
                    completed_components = summary.completed,
                    status = CASE
                        WHEN ds.status = 'failed' THEN ds.status
                        WHEN summary.total = 0 THEN ds.status
                        WHEN summary.resolved < summary.total THEN 'running'
                        WHEN summary.failed > 0 THEN 'completed_partial'
                        ELSE 'completed'
                    END,
                    completed_at = CASE
                        WHEN ds.status = 'failed' THEN COALESCE(ds.completed_at, now())
                        WHEN summary.total > 0 AND summary.resolved = summary.total THEN now()
                        ELSE NULL
                    END
                FROM summary
                WHERE ds.id = $1
                RETURNING ds.status, ds.decision_id
            ), dec_update AS (
                UPDATE decisions
                SET status = 'completed', updated_at = now()
                FROM sim_update
                WHERE decisions.id = sim_update.decision_id
                  AND sim_update.status IN ('completed', 'completed_partial')
                RETURNING decisions.id
            )
            SELECT status FROM sim_update",
        )
        .bind(sim_id)
        .fetch_one(&mut **tx)
        .await?;
        Ok(row.0)
    }

    pub async fn list_components(&self, sim_id: Uuid) -> Result<Vec<SimulationComponent>> {
        let rows = sqlx::query_as::<_, SimulationComponent>(
            "SELECT id, simulation_id, component_key, component_type, display_name, status, path, phase,
                    error_code, error_message, COALESCE(metadata, '{}'::jsonb) AS metadata, created_at, started_at, completed_at, updated_at
             FROM simulation_components
             WHERE simulation_id = $1
             ORDER BY component_type, component_key",
        )
        .bind(sim_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}

pub fn nullable_string(s: &str) -> Option<&str> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}
