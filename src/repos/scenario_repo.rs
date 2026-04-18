use crate::models::*;
use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Clone)]
pub struct ScenarioRepository {
    pool: PgPool,
}

impl ScenarioRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn insert_scenario_plan(&self, plan: &ScenarioPlan) -> Result<()> {
        sqlx::query(
            "INSERT INTO scenario_plans
               (id, simulation_id, path_a, path_b, shared_context, raw_ai_response, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, now())",
        )
        .bind(plan.id)
        .bind(plan.simulation_id)
        .bind(&plan.path_a)
        .bind(&plan.path_b)
        .bind(&plan.shared_context)
        .bind(&plan.raw_ai_response)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_scenario_plan_by_simulation_id(&self, sim_id: Uuid) -> Result<ScenarioPlan> {
        let sp = sqlx::query_as::<_, ScenarioPlan>(
            "SELECT id, simulation_id, path_a, path_b, shared_context, raw_ai_response, created_at
             FROM scenario_plans WHERE simulation_id = $1",
        )
        .bind(sim_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(sp)
    }
}
