use crate::app_state::AppState;
use crate::handlers::{write_billing_error, write_error, write_json};
use crate::jobs::{self, ScenarioPlannerArgs};
use crate::middleware::AuthUser;
use crate::models::{self, ClarifyingQuestion, Decision, DecisionSimulation};
use crate::prompts::{self, SimulationContext};
use crate::providers::TextRequest;
use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value as JsonValue};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct CreateDecisionReq {
    pub decision_text: String,
    #[serde(default)]
    pub input_method: String,
    #[serde(default)]
    pub time_horizon_months: i32,
}

pub async fn post_decision(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    Json(req): Json<CreateDecisionReq>,
) -> Response {
    if req.decision_text.is_empty() {
        tracing::debug!(user_id = %user_id, "decisions.create: missing decision_text");
        return write_error(StatusCode::BAD_REQUEST, "decision_text is required");
    }
    let time_horizon = if req.time_horizon_months <= 0 { 12 } else { req.time_horizon_months };
    let method = if req.input_method.is_empty() { "text".into() } else { req.input_method };
    tracing::info!(
        user_id = %user_id,
        input_method = %method,
        time_horizon_months = time_horizon,
        text_length = req.decision_text.len(),
        "decisions.create: creating decision"
    );

    let decision = Decision {
        id: Uuid::new_v4(),
        user_id,
        decision_text: req.decision_text.clone(),
        input_method: method,
        time_horizon_months: time_horizon,
        status: models::decision_status::DRAFT.into(),
        category: String::new(),
        severity: 0,
        reversibility: String::new(),
        share_token: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    if let Err(e) = state.decision_repo.create_decision(&decision).await {
        tracing::error!(user_id = %user_id, error = ?e, "decisions.create: failed to persist decision");
        return write_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to create decision");
    }
    tracing::debug!(decision_id = %decision.id, "decisions.create: persisted, generating clarifying questions");

    // Generate clarifying questions synchronously.
    let profile = state.user_repo.get_profile_by_user_id(user_id).await.ok();
    let life_state = state.user_repo.build_life_state(user_id).await.unwrap_or_else(|_| models::LifeState::default_state());
    let life_story = state.user_repo.get_life_story_by_user_id(user_id).await.ok();
    let mut sctx = SimulationContext::default();
    sctx.user = profile.clone();
    sctx.life_state = life_state;
    sctx.life_story = life_story;
    sctx.decision = Some(decision.clone());
    sctx.time_horizon_months = time_horizon;

    let (sys, user) = state.prompt_builder.build_text_prompt(prompts::TASK_ONBOARDING_QUESTIONS, &sctx);
    let r = state
        .text_provider
        .generate_text(&TextRequest {
            system_prompt: sys,
            user_prompt: user,
            max_tokens: 2500,
            temperature: 0.5,
            json_mode: true,
            ..Default::default()
        })
        .await;
    let mut questions: Vec<ClarifyingQuestion> = vec![];
    if let Ok(resp) = r {
        if let Ok(parsed) = serde_json::from_str::<JsonValue>(&resp.content) {
            if let Some(arr) = parsed.get("questions").and_then(|v| v.as_array()) {
                for (i, q) in arr.iter().enumerate() {
                    if let Some(text) = q.get("question_text").and_then(|v| v.as_str()) {
                        questions.push(ClarifyingQuestion {
                            id: Uuid::new_v4(),
                            decision_id: decision.id,
                            question_text: text.to_string(),
                            answer_text: String::new(),
                            answer_method: String::new(),
                            sort_order: q.get("sort_order").and_then(|v| v.as_i64()).unwrap_or((i + 1) as i64) as i32,
                            created_at: chrono::Utc::now(),
                        });
                    }
                }
            }
            if let Some(cat) = parsed.get("category").and_then(|v| v.as_str()) {
                let _ = state.decision_repo.update_category(decision.id, cat).await;
            }
            let severity = parsed.get("severity").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let reversibility = parsed
                .get("reversibility")
                .and_then(|v| v.as_str())
                .unwrap_or("reversible_with_cost");
            let _ = state
                .decision_repo
                .update_severity_and_reversibility(decision.id, severity, reversibility)
                .await;
        }
    }
    let question_count = questions.len();
    if let Err(e) = state.decision_repo.bulk_insert_clarifying_questions(&questions).await {
        tracing::warn!(error = ?e, decision_id = %decision.id, "decisions.create: failed to persist clarifying questions");
    }
    let _ = state
        .decision_repo
        .update_decision_status(decision.id, models::decision_status::CLARIFYING)
        .await;
    tracing::info!(
        decision_id = %decision.id,
        question_count,
        "decisions.create: created with clarifying questions"
    );

    write_json(
        StatusCode::CREATED,
        json!({ "decision_id": decision.id, "clarifying_questions": questions }),
    )
}

#[derive(Deserialize)]
pub struct PostAnswer {
    pub question_id: Uuid,
    pub answer_text: String,
    #[serde(default)]
    pub answer_method: String,
}

#[derive(Deserialize)]
pub struct PostAnswersReq {
    pub answers: Vec<PostAnswer>,
    #[serde(default)]
    pub time_horizon_months: i32,
    #[serde(default)]
    pub skip_questions: bool,
}

pub async fn post_answers(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    Path(decision_id): Path<Uuid>,
    Json(req): Json<PostAnswersReq>,
) -> Response {
    tracing::info!(
        user_id = %user_id,
        decision_id = %decision_id,
        answer_count = req.answers.len(),
        time_horizon_months = req.time_horizon_months,
        skip_questions = req.skip_questions,
        "decisions.answers: submitting answers and dispatching simulation"
    );
    if req.time_horizon_months > 0 {
        if let Err(e) = state
            .decision_repo
            .update_time_horizon_months(decision_id, req.time_horizon_months)
            .await
        {
            tracing::error!(decision_id = %decision_id, error = ?e, "decisions.answers: failed to update time horizon");
            return write_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to update time horizon");
        }
    }

    let qs = match state.decision_repo.get_clarifying_questions_by_decision_id(decision_id).await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(decision_id = %decision_id, error = ?e, "decisions.answers: failed to load clarifying questions");
            return write_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load clarifying questions");
        }
    };
    let valid_ids: std::collections::HashSet<Uuid> = qs.iter().map(|q| q.id).collect();
    for a in &req.answers {
        if !valid_ids.contains(&a.question_id) {
            return write_error(StatusCode::BAD_REQUEST, "question does not belong to this decision");
        }
        let method = if a.answer_method.is_empty() { "text" } else { &a.answer_method };
        if let Err(e) = state
            .decision_repo
            .update_clarifying_answer(a.question_id, &a.answer_text, method)
            .await
        {
            tracing::error!(error = ?e, "post_answers: failed to save answer");
            return write_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to save answer");
        }
    }

    if !req.skip_questions && !qs.is_empty() {
        let answered = req.answers.iter().filter(|a| !a.answer_text.is_empty()).count();
        if answered == 0 {
            return write_error(
                StatusCode::BAD_REQUEST,
                "at least one clarifying question must be answered; set skip_questions=true to bypass",
            );
        }
    }

    // Dispatch simulation: check entitlement and enqueue the scenario planner.
    match dispatch_simulation(&state, user_id, decision_id).await {
        Ok(sim_id) => {
            tracing::info!(
                user_id = %user_id,
                decision_id = %decision_id,
                simulation_id = %sim_id,
                "decisions.answers: simulation dispatched"
            );
            write_json(StatusCode::OK, json!({ "simulation_id": sim_id }))
        }
        Err(DispatchError::Entitlement { code, message }) => {
            tracing::warn!(
                user_id = %user_id,
                decision_id = %decision_id,
                code = %code,
                "decisions.answers: simulation entitlement blocked"
            );
            write_billing_error(&code, &message)
        }
        Err(DispatchError::Gate { code, message }) => {
            tracing::info!(
                user_id = %user_id,
                decision_id = %decision_id,
                code = %code,
                "decisions.answers: simulation blocked by gate"
            );
            (
                StatusCode::FORBIDDEN,
                Json(json!({"error_code": code, "message": message})),
            )
                .into_response()
        }
        Err(DispatchError::Other(e)) => {
            tracing::error!(
                user_id = %user_id,
                decision_id = %decision_id,
                error = ?e,
                "decisions.answers: failed to dispatch simulation"
            );
            write_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to dispatch simulation")
        }
    }
}

#[derive(Debug)]
enum DispatchError {
    Entitlement { code: String, message: String },
    Gate { code: String, message: String },
    Other(anyhow::Error),
}

async fn dispatch_simulation(
    state: &AppState,
    user_id: Uuid,
    decision_id: Uuid,
) -> Result<Uuid, DispatchError> {
    // Cinematic context gate.
    let gate = state
        .user_repo
        .get_cinematic_context_status(user_id)
        .await
        .map_err(DispatchError::Other)?;
    if !gate {
        return Err(DispatchError::Gate {
            code: models::gate_code::CINEMATIC_CONTEXT_REQUIRED.into(),
            message: "cinematic context is required before the first cinematic simulation".into(),
        });
    }

    // Consume entitlement (only if billing enabled).
    if state.billing.billing_enabled() {
        let mut tx = state.pool.begin().await.map_err(|e| DispatchError::Other(e.into()))?;
        let (_sub, _used_extra) = state
            .subscription_repo
            .consume_cinematic_entitlement_tx(&mut tx, user_id)
            .await
            .map_err(|e| DispatchError::Entitlement { code: e.code, message: e.message })?;
        tx.commit().await.map_err(|e| DispatchError::Other(e.into()))?;
    }

    // Build the full simulation context (frozen snapshot).
    let profile = state
        .user_repo
        .get_profile_by_user_id(user_id)
        .await
        .map_err(DispatchError::Other)?;
    let life_state = state
        .user_repo
        .build_life_state(user_id)
        .await
        .map_err(DispatchError::Other)?;
    let behavioral = crate::models::resolve_behavioral_profile(&profile);
    let financial_profile = crate::models::resolve_financial_profile(&profile);
    let life_context = crate::models::resolve_life_context_profile(&profile);
    let decision = state
        .decision_repo
        .get_decision_by_id(decision_id)
        .await
        .map_err(DispatchError::Other)?;
    let fact_sheet = crate::financial::build_financial_fact_sheet(&profile, &life_state, &financial_profile);
    let relevance = crate::financial::decision_financial_relevance(&decision.category);
    let neutral = crate::financial::is_financially_neutral(relevance);

    let snapshot = json!({
        "profile": profile,
        "behavioral_profile": behavioral,
        "financial_profile": financial_profile,
        "life_context_profile": life_context,
        "financial_fact_sheet": fact_sheet,
        "financial_relevance": relevance.as_str(),
        "financially_neutral": neutral,
    });
    let life_state_snap = serde_json::to_value(&life_state).unwrap_or(JsonValue::Null);

    let run_number = state
        .simulation_repo
        .get_max_run_number(decision_id)
        .await
        .unwrap_or(0)
        + 1;

    let sim_id = Uuid::new_v4();
    let sim = DecisionSimulation {
        id: sim_id,
        decision_id,
        user_id,
        status: models::simulation_status::RUNNING.into(),
        total_components: 2, // scenario_plan + assumptions; video phases added later
        completed_components: 0,
        run_type: models::simulation_run_type::CINEMATIC.into(),
        user_context_snapshot: snapshot,
        life_state_snapshot: life_state_snap,
        data_completeness: life_state.completeness,
        started_at: Some(chrono::Utc::now()),
        completed_at: None,
        created_at: chrono::Utc::now(),
        parent_simulation_id: None,
        run_number,
        assumption_overrides: JsonValue::Null,
        assumptions_calibrated_at: None,
    };
    state
        .simulation_repo
        .create_simulation(&sim)
        .await
        .map_err(DispatchError::Other)?;

    // Register simulation components.
    let components = vec![
        models::SimulationComponent {
            id: Uuid::nil(),
            simulation_id: sim_id,
            component_key: "scenario_plan".into(),
            component_type: models::simulation_component_type::SCENARIO_PLAN.into(),
            display_name: "Scenario plan".into(),
            status: models::simulation_component_status::PENDING.into(),
            path: None,
            phase: None,
            error_code: None,
            error_message: None,
            metadata: JsonValue::Null,
            created_at: chrono::Utc::now(),
            started_at: None,
            completed_at: None,
            updated_at: chrono::Utc::now(),
        },
        models::SimulationComponent {
            id: Uuid::nil(),
            simulation_id: sim_id,
            component_key: "assumptions".into(),
            component_type: models::simulation_component_type::ASSUMPTIONS.into(),
            display_name: "Assumptions".into(),
            status: models::simulation_component_status::PENDING.into(),
            path: None,
            phase: None,
            error_code: None,
            error_message: None,
            metadata: JsonValue::Null,
            created_at: chrono::Utc::now(),
            started_at: None,
            completed_at: None,
            updated_at: chrono::Utc::now(),
        },
    ];
    state
        .components_repo
        .upsert_simulation_components(&components)
        .await
        .map_err(DispatchError::Other)?;

    // Mark decision simulating.
    let _ = state
        .decision_repo
        .update_decision_status(decision_id, models::decision_status::SIMULATING)
        .await;

    // Enqueue the scenario planner.
    state
        .job_client
        .insert(
            jobs::KIND_SCENARIO_PLANNER,
            &ScenarioPlannerArgs { simulation_id: sim_id },
        )
        .await
        .map_err(DispatchError::Other)?;

    Ok(sim_id)
}

pub async fn list_decisions(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
) -> Response {
    match state.decision_repo.list_decisions_by_user_id(user_id).await {
        Ok(decisions) => {
            let total = decisions.len();
            tracing::debug!(user_id = %user_id, total, "decisions.list: returning decisions");
            write_json(
                StatusCode::OK,
                json!({ "decisions": decisions, "total": total, "limit": 0, "offset": 0 }),
            )
        }
        Err(e) => {
            tracing::error!(user_id = %user_id, error = ?e, "decisions.list: failed");
            write_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to list decisions")
        }
    }
}

pub async fn get_decision(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Response {
    let decision = match state.decision_repo.get_decision_by_id(id).await {
        Ok(d) => d,
        Err(e) => {
            tracing::debug!(user_id = %user_id, decision_id = %id, error = ?e, "decisions.get: not found");
            return write_error(StatusCode::NOT_FOUND, "decision not found");
        }
    };
    let questions = state
        .decision_repo
        .get_clarifying_questions_by_decision_id(id)
        .await
        .unwrap_or_default();
    tracing::debug!(decision_id = %id, status = %decision.status, question_count = questions.len(), "decisions.get: returning");
    write_json(StatusCode::OK, json!({ "decision": decision, "questions": questions }))
}
