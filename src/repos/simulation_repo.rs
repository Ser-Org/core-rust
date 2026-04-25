use crate::models::*;
use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde_json::Value as JsonValue;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Clone)]
pub struct SimulationRepository {
    pool: PgPool,
}

impl SimulationRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn create_simulation(&self, sim: &DecisionSimulation) -> Result<()> {
        sqlx::query(
            "INSERT INTO decision_simulations
               (id, decision_id, user_id, status, total_components, completed_components, run_type,
                user_context_snapshot, life_state_snapshot, data_completeness, started_at,
                parent_simulation_id, run_number, assumption_overrides, assumptions_calibrated_at,
                created_at)
             VALUES ($1, $2, $3, $4, $5, 0, $6, $7, $8, $9, $10, $11, $12, $13, $14, now())",
        )
        .bind(sim.id)
        .bind(sim.decision_id)
        .bind(sim.user_id)
        .bind(&sim.status)
        .bind(sim.total_components)
        .bind(&sim.run_type)
        .bind(&sim.user_context_snapshot)
        .bind(&sim.life_state_snapshot)
        .bind(sim.data_completeness)
        .bind(sim.started_at)
        .bind(sim.parent_simulation_id)
        .bind(sim.run_number)
        .bind(&sim.assumption_overrides)
        .bind(sim.assumptions_calibrated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_simulation_by_id(&self, id: Uuid) -> Result<DecisionSimulation> {
        let s = sqlx::query_as::<_, DecisionSimulation>(
            "SELECT id, decision_id, user_id, status, total_components, completed_components,
                    run_type, user_context_snapshot, life_state_snapshot, data_completeness::float8 AS data_completeness, started_at, completed_at, created_at,
                    parent_simulation_id, run_number, COALESCE(assumption_overrides, 'null'::jsonb) AS assumption_overrides, assumptions_calibrated_at
             FROM decision_simulations WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await?;
        Ok(s)
    }

    pub async fn get_simulation_by_decision_id(
        &self,
        decision_id: Uuid,
    ) -> Result<DecisionSimulation> {
        let s = sqlx::query_as::<_, DecisionSimulation>(
            "SELECT id, decision_id, user_id, status, total_components, completed_components,
                    run_type, user_context_snapshot, life_state_snapshot, data_completeness::float8 AS data_completeness, started_at, completed_at, created_at,
                    parent_simulation_id, run_number, COALESCE(assumption_overrides, 'null'::jsonb) AS assumption_overrides, assumptions_calibrated_at
             FROM decision_simulations WHERE decision_id = $1
             ORDER BY run_number DESC LIMIT 1",
        )
        .bind(decision_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(s)
    }

    pub async fn list_simulation_versions(
        &self,
        decision_id: Uuid,
    ) -> Result<Vec<DecisionSimulation>> {
        let rows = sqlx::query_as::<_, DecisionSimulation>(
            "SELECT id, decision_id, user_id, status, total_components, completed_components,
                    run_type,
                    '{}'::jsonb AS user_context_snapshot,
                    '{}'::jsonb AS life_state_snapshot,
                    data_completeness::float8 AS data_completeness, started_at, completed_at, created_at,
                    parent_simulation_id, run_number,
                    'null'::jsonb AS assumption_overrides, NULL::timestamptz AS assumptions_calibrated_at
             FROM decision_simulations WHERE decision_id = $1
             ORDER BY run_number ASC",
        )
        .bind(decision_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn get_max_run_number(&self, decision_id: Uuid) -> Result<i32> {
        let row: (i32,) = sqlx::query_as(
            "SELECT COALESCE(MAX(run_number), 0) FROM decision_simulations WHERE decision_id = $1",
        )
        .bind(decision_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    pub async fn increment_completed_components(&self, id: Uuid) -> Result<String> {
        let row: (String,) = sqlx::query_as(
            "WITH sim_update AS (
                UPDATE decision_simulations
                SET completed_components = completed_components + 1,
                    status = CASE
                        WHEN status = 'completed_partial' THEN 'completed_partial'
                        WHEN completed_components + 1 = total_components THEN 'completed'
                        ELSE status
                    END,
                    completed_at = CASE
                        WHEN status != 'completed_partial' AND completed_components + 1 = total_components THEN now()
                        ELSE completed_at
                    END
                WHERE id = $1
                RETURNING status, decision_id
            ), dec_update AS (
                UPDATE decisions SET status = 'completed', updated_at = now()
                FROM sim_update
                WHERE decisions.id = sim_update.decision_id
                  AND sim_update.status = 'completed'
                RETURNING decisions.id
            )
            SELECT status FROM sim_update",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    pub async fn increment_failed_component(&self, id: Uuid) -> Result<String> {
        let row: (String,) = sqlx::query_as(
            "WITH sim_update AS (
                UPDATE decision_simulations
                SET completed_components = completed_components + 1,
                    status = 'completed_partial',
                    completed_at = CASE
                        WHEN completed_components + 1 = total_components THEN now()
                        ELSE completed_at
                    END
                WHERE id = $1
                RETURNING status, decision_id, completed_components, total_components
            ), dec_update AS (
                UPDATE decisions SET status = 'completed', updated_at = now()
                FROM sim_update
                WHERE decisions.id = sim_update.decision_id
                  AND sim_update.completed_components = sim_update.total_components
                RETURNING decisions.id
            )
            SELECT status FROM sim_update",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    pub async fn update_total_components(&self, id: Uuid, additional: i32) -> Result<()> {
        sqlx::query(
            "UPDATE decision_simulations SET total_components = total_components + $1 WHERE id = $2",
        )
        .bind(additional)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_simulation_status(&self, id: Uuid, status: &str) -> Result<()> {
        let res = sqlx::query("UPDATE decision_simulations SET status = $1 WHERE id = $2")
            .bind(status)
            .bind(id)
            .execute(&self.pool)
            .await?;
        if res.rows_affected() == 0 {
            return Err(anyhow!("simulation {} not found", id));
        }
        Ok(())
    }

    pub async fn mark_assumptions_calibrated_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        sim_id: Uuid,
    ) -> Result<DateTime<Utc>> {
        let row: (DateTime<Utc>,) = sqlx::query_as(
            "UPDATE decision_simulations
             SET assumptions_calibrated_at = now()
             WHERE id = $1 AND assumptions_calibrated_at IS NULL
             RETURNING assumptions_calibrated_at",
        )
        .bind(sim_id)
        .fetch_one(&mut **tx)
        .await?;
        Ok(row.0)
    }

    pub async fn upsert_dashboard_snapshot(
        &self,
        id: Uuid,
        user_id: Uuid,
        financial_trajectory: &JsonValue,
        life_momentum_score: &JsonValue,
        probability_outlook: &JsonValue,
        narrative_summary: &str,
        raw_ai_response: &str,
        generated_at: DateTime<Utc>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO scout_dashboard_snapshots
               (id, user_id, financial_trajectory, life_momentum_score, probability_outlook,
                narrative_summary, raw_ai_response, generated_at, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, now())
             ON CONFLICT (user_id) DO UPDATE SET
               financial_trajectory = EXCLUDED.financial_trajectory,
               life_momentum_score = EXCLUDED.life_momentum_score,
               probability_outlook = EXCLUDED.probability_outlook,
               narrative_summary = EXCLUDED.narrative_summary,
               raw_ai_response = EXCLUDED.raw_ai_response,
               generated_at = EXCLUDED.generated_at",
        )
        .bind(id)
        .bind(user_id)
        .bind(financial_trajectory)
        .bind(life_momentum_score)
        .bind(probability_outlook)
        .bind(narrative_summary)
        .bind(raw_ai_response)
        .bind(generated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn bulk_insert_assumptions(
        &self,
        assumptions: &[SimulationAssumption],
    ) -> Result<()> {
        if assumptions.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        for a in assumptions {
            sqlx::query(
                "INSERT INTO simulation_assumptions
                   (id, simulation_id, description, confidence, source, kind, grounding, evidence_refs, category, editable, profile_field, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, now(), now())",
            )
            .bind(a.id)
            .bind(a.simulation_id)
            .bind(&a.description)
            .bind(a.confidence)
            .bind(&a.source)
            .bind(&a.kind)
            .bind(&a.grounding)
            .bind(&a.evidence_refs)
            .bind(&a.category)
            .bind(a.editable)
            .bind(&a.profile_field)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn bulk_insert_risks(&self, risks: &[SimulationRisk]) -> Result<()> {
        if risks.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        for r in risks {
            sqlx::query(
                "INSERT INTO simulation_risks
                   (id, simulation_id, description, likelihood, impact, category, linked_assumption_ids, mitigation_hint, created_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, now())",
            )
            .bind(r.id)
            .bind(r.simulation_id)
            .bind(&r.description)
            .bind(&r.likelihood)
            .bind(&r.impact)
            .bind(&r.category)
            .bind(&r.linked_assumption_ids)
            .bind(&r.mitigation_hint)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn get_assumptions_by_simulation_id(
        &self,
        sim_id: Uuid,
    ) -> Result<Vec<SimulationAssumption>> {
        let rows = sqlx::query_as::<_, SimulationAssumption>(
            "SELECT id, simulation_id, description, confidence::float8 AS confidence,
                    COALESCE(source, '') AS source, COALESCE(kind, '') AS kind, COALESCE(grounding, '') AS grounding,
                    COALESCE(evidence_refs, '{}'::text[]) AS evidence_refs,
                    category, editable, user_override_value, original_confidence::float8 AS original_confidence, profile_field, created_at, updated_at
             FROM simulation_assumptions WHERE simulation_id = $1
             ORDER BY created_at",
        )
        .bind(sim_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn get_risks_by_simulation_id(&self, sim_id: Uuid) -> Result<Vec<SimulationRisk>> {
        let rows = sqlx::query_as::<_, SimulationRisk>(
            "SELECT id, simulation_id, description, likelihood, impact,
                    COALESCE(category, '') AS category,
                    COALESCE(linked_assumption_ids, '{}'::uuid[]) AS linked_assumption_ids,
                    COALESCE(mitigation_hint, '') AS mitigation_hint, created_at
             FROM simulation_risks WHERE simulation_id = $1
             ORDER BY created_at",
        )
        .bind(sim_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn get_assumption_by_id_for_simulation(
        &self,
        id: Uuid,
        simulation_id: Uuid,
    ) -> Result<SimulationAssumption> {
        let a = sqlx::query_as::<_, SimulationAssumption>(
            "SELECT id, simulation_id, description, confidence::float8 AS confidence,
                    COALESCE(source, '') AS source, COALESCE(kind, '') AS kind, COALESCE(grounding, '') AS grounding,
                    COALESCE(evidence_refs, '{}'::text[]) AS evidence_refs,
                    category, editable, user_override_value, original_confidence::float8 AS original_confidence, profile_field, created_at, updated_at
             FROM simulation_assumptions WHERE id = $1 AND simulation_id = $2",
        )
        .bind(id)
        .bind(simulation_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(a)
    }

    pub async fn update_assumption_for_simulation(
        &self,
        id: Uuid,
        simulation_id: Uuid,
        override_value: Option<&str>,
        confidence: Option<f64>,
    ) -> Result<bool> {
        let res = sqlx::query(
            "UPDATE simulation_assumptions
             SET user_override_value = COALESCE($2, user_override_value),
                 original_confidence = CASE WHEN original_confidence IS NULL THEN confidence ELSE original_confidence END,
                 confidence = COALESCE($3, confidence),
                 updated_at = now()
             WHERE id = $1 AND simulation_id = $4",
        )
        .bind(id)
        .bind(override_value)
        .bind(confidence)
        .bind(simulation_id)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    pub async fn update_assumption_for_user(
        &self,
        id: Uuid,
        user_id: Uuid,
        override_value: Option<&str>,
        confidence: Option<f64>,
    ) -> Result<bool> {
        let res = sqlx::query(
            "UPDATE simulation_assumptions AS a
             SET user_override_value = COALESCE($2, a.user_override_value),
                 original_confidence = CASE WHEN a.original_confidence IS NULL THEN a.confidence ELSE a.original_confidence END,
                 confidence = COALESCE($3, a.confidence),
                 updated_at = now()
             FROM decision_simulations AS ds
             WHERE a.id = $1
               AND a.simulation_id = ds.id
               AND ds.user_id = $4",
        )
        .bind(id)
        .bind(override_value)
        .bind(confidence)
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }
}
