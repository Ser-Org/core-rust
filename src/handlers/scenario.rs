use crate::app_state::AppState;
use crate::handlers::{write_billing_error, write_error, write_json};
use crate::jobs::{self, ScenarioPlannerArgs};
use crate::middleware::AuthUser;
use crate::models;
use crate::providers::TextRequest;
use crate::repos::UserCalibrationProfilePatch;
use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    response::Response,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value as JsonValue};
use std::time::Duration;
use uuid::Uuid;

pub async fn get_scenario_plan(
    State(state): State<AppState>,
    Extension(_): Extension<AuthUser>,
    Path(decision_id): Path<Uuid>,
) -> Response {
    let sim = match state
        .simulation_repo
        .get_simulation_by_decision_id(decision_id)
        .await
    {
        Ok(s) => s,
        Err(_) => return write_error(StatusCode::NOT_FOUND, "simulation not found"),
    };
    let plan = match state
        .scenario_repo
        .get_scenario_plan_by_simulation_id(sim.id)
        .await
    {
        Ok(p) => p,
        Err(_) => {
            return write_json(
                StatusCode::OK,
                json!({
                    "status": "generating",
                    "simulation_id": sim.id,
                    "scenario_plan": JsonValue::Null,
                }),
            );
        }
    };
    write_json(
        StatusCode::OK,
        json!({
            "status": "ready",
            "simulation_id": plan.simulation_id,
            "scenario_plan": {
                "id": plan.id,
                "simulation_id": plan.simulation_id,
                "path_a": plan.path_a,
                "path_b": plan.path_b,
                "shared_context": plan.shared_context,
                "created_at": plan.created_at,
            },
        }),
    )
}

pub async fn get_simulation_media(
    State(state): State<AppState>,
    Extension(_): Extension<AuthUser>,
    Path(decision_id): Path<Uuid>,
) -> Response {
    let sim = match state
        .simulation_repo
        .get_simulation_by_decision_id(decision_id)
        .await
    {
        Ok(s) => s,
        Err(_) => return write_error(StatusCode::NOT_FOUND, "simulation not found"),
    };
    let media = state
        .media_repo
        .get_media_by_simulation_and_scenario(sim.id)
        .await
        .unwrap_or_default();
    let components = state
        .components_repo
        .list_components(sim.id)
        .await
        .unwrap_or_default();
    // Build (path, phase) -> status map from video_clip components.
    let status_map: std::collections::HashMap<(String, i32), String> = components
        .iter()
        .filter(|c| c.component_type == "video_clip")
        .filter_map(|c| {
            let p = c.path.as_ref()?;
            let ph = c.phase?;
            Some(((p.clone(), ph), c.status.clone()))
        })
        .collect();

    let mut clips = Vec::with_capacity(media.len());
    for m in media {
        let signed = state
            .object_store
            .get_signed_url(
                &state.cfg.s3_bucket,
                &m.storage_path,
                Duration::from_secs(3600),
            )
            .await
            .unwrap_or(m.storage_url.clone());
        // Frontend expects path as 'a'|'b'; DB stores 'path_a'|'path_b'.
        let db_path = m.scenario_path.clone().unwrap_or_default();
        let normalized_path = db_path
            .strip_prefix("path_")
            .unwrap_or(&db_path)
            .to_string();
        let phase = m.scenario_phase.unwrap_or(0);
        let status = status_map
            .get(&(db_path, phase))
            .cloned()
            .unwrap_or_else(|| "completed".to_string());
        clips.push(json!({
            "id": m.id,
            "path": normalized_path,
            "phase": phase,
            "status": status,
            "url": signed,
            "clip_role": m.clip_role,
            "created_at": m.created_at,
        }));
    }
    write_json(StatusCode::OK, json!({ "clips": clips }))
}

pub async fn get_simulation_progress(
    State(state): State<AppState>,
    Extension(_): Extension<AuthUser>,
    Path(decision_id): Path<Uuid>,
) -> Response {
    let sim = match state
        .simulation_repo
        .get_simulation_by_decision_id(decision_id)
        .await
    {
        Ok(s) => s,
        Err(_) => return write_json(StatusCode::OK, json!({ "status": "not_started" })),
    };
    let components = state
        .components_repo
        .list_components(sim.id)
        .await
        .unwrap_or_default();

    // Default stage entries so the frontend can always read `.status` without crashing.
    let pending = || json!({ "status": models::simulation_component_status::PENDING });
    let mut scenario_plan = pending();
    let mut assumptions = pending();
    let mut character_palette: JsonValue = JsonValue::Null;

    let mut video_total = 0i32;
    let mut video_pending = 0i32;
    let mut video_running = 0i32;
    let mut video_completed = 0i32;
    let mut video_failed = 0i32;
    let mut video_clips: Vec<JsonValue> = Vec::new();

    let mut pending_n = 0i32;
    let mut running_n = 0i32;
    let mut completed_n = 0i32;
    let mut failed_n = 0i32;

    for c in &components {
        match c.status.as_str() {
            "pending" => pending_n += 1,
            "running" => running_n += 1,
            "completed" => completed_n += 1,
            "failed" => failed_n += 1,
            _ => {}
        }
        let mut entry = json!({ "status": c.status });
        if let Some(code) = &c.error_code {
            entry["error_code"] = json!(code);
        }
        if let Some(msg) = &c.error_message {
            entry["error_message"] = json!(msg);
        }

        match c.component_type.as_str() {
            "scenario_plan" => scenario_plan = entry,
            "assumptions" => assumptions = entry,
            "character_palette" => character_palette = entry,
            "video_clip" => {
                video_total += 1;
                match c.status.as_str() {
                    "pending" => video_pending += 1,
                    "running" => video_running += 1,
                    "completed" => video_completed += 1,
                    "failed" => video_failed += 1,
                    _ => {}
                }
                // Frontend expects path as 'a' | 'b'; DB stores 'path_a' | 'path_b'.
                let normalized_path = c
                    .path
                    .as_deref()
                    .map(|p| p.strip_prefix("path_").unwrap_or(p))
                    .unwrap_or("");
                let mut clip = json!({
                    "path": normalized_path,
                    "phase": c.phase.unwrap_or(0),
                    "status": c.status,
                });
                if let Some(code) = &c.error_code {
                    clip["error_code"] = json!(code);
                }
                if let Some(msg) = &c.error_message {
                    clip["error_message"] = json!(msg);
                }
                video_clips.push(clip);
            }
            _ => {}
        }
    }

    let video_resolved = video_completed + video_failed;
    let resolved_n = completed_n + failed_n;

    write_json(
        StatusCode::OK,
        json!({
            "simulation_id": sim.id,
            "status": sim.status,
            "run_type": sim.run_type,
            "total_components": sim.total_components,
            "completed_components": sim.completed_components,
            "failed_components": failed_n,
            "pending_components": pending_n,
            "running_components": running_n,
            "resolved_components": resolved_n,
            "components": {
                "scenario_plan": scenario_plan,
                "assumptions": assumptions,
                "character_palette": character_palette,
                "video_clips": {
                    "total": video_total,
                    "pending": video_pending,
                    "running": video_running,
                    "completed": video_completed,
                    "failed": video_failed,
                    "resolved": video_resolved,
                    "clips": video_clips,
                },
            },
        }),
    )
}

pub async fn get_assumptions(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    Path(sim_id): Path<Uuid>,
) -> Response {
    let sim = match state.simulation_repo.get_simulation_by_id(sim_id).await {
        Ok(s) => s,
        Err(_) => return write_error(StatusCode::NOT_FOUND, "simulation not found"),
    };
    if sim.user_id != user_id {
        return write_error(StatusCode::NOT_FOUND, "simulation not found");
    }

    let assumptions = state
        .simulation_repo
        .get_assumptions_by_simulation_id(sim_id)
        .await
        .unwrap_or_default();
    let risks = state
        .simulation_repo
        .get_risks_by_simulation_id(sim_id)
        .await
        .unwrap_or_default();
    let locked = sim.assumptions_calibrated_at.is_some();
    let used_at = sim.assumptions_calibrated_at;
    write_json(
        StatusCode::OK,
        json!({
            "assumptions": assumptions,
            "risks": risks,
            "assumptions_locked": locked,
            "can_teach_vidente": !locked,
            "teach_vidente_used_at": used_at,
        }),
    )
}

#[derive(Deserialize)]
pub struct UpdateAssumptionReq {
    pub user_override_value: Option<String>,
    pub confidence: Option<f64>,
}

pub async fn update_assumption(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    Path((sim_id, aid)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateAssumptionReq>,
) -> Response {
    let sim = match state.simulation_repo.get_simulation_by_id(sim_id).await {
        Ok(s) => s,
        Err(_) => return write_error(StatusCode::NOT_FOUND, "simulation not found"),
    };
    if sim.user_id != user_id {
        return write_error(StatusCode::NOT_FOUND, "simulation not found");
    }

    match state
        .simulation_repo
        .update_assumption_for_simulation(
            aid,
            sim_id,
            req.user_override_value.as_deref(),
            req.confidence,
        )
        .await
    {
        Ok(true) => write_json(StatusCode::OK, json!({"status": "ok"})),
        Ok(false) => write_error(StatusCode::NOT_FOUND, "assumption not found"),
        Err(e) => {
            tracing::error!(error = ?e, "update_assumption: failed to update assumption");
            write_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to update assumption",
            )
        }
    }
}

#[derive(Deserialize, Default)]
pub struct CalibrateReq {
    #[serde(default)]
    pub edited_assumption_ids: Vec<String>,
}

pub async fn calibrate_assumptions(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    Path(sim_id): Path<Uuid>,
    body: Option<Json<CalibrateReq>>,
) -> Response {
    let sim = match state.simulation_repo.get_simulation_by_id(sim_id).await {
        Ok(s) => s,
        Err(_) => return write_error(StatusCode::NOT_FOUND, "simulation not found"),
    };
    if sim.user_id != user_id {
        return write_error(StatusCode::NOT_FOUND, "simulation not found");
    }

    let edited_ids = body.map(|b| b.0.edited_assumption_ids).unwrap_or_default();

    // Load edited assumptions and build typed profile patch.
    let mut patch = UserCalibrationProfilePatch::default();
    let mut applied_ids: Vec<String> = Vec::new();
    let mut skipped_ids: Vec<String> = Vec::new();
    let mut updated_fields: Vec<String> = Vec::new();
    let mut applied_details: Vec<(String, String)> = Vec::new();

    for id_str in &edited_ids {
        let uuid = match Uuid::parse_str(id_str) {
            Ok(u) => u,
            Err(_) => {
                skipped_ids.push(id_str.clone());
                continue;
            }
        };
        let a = match state
            .simulation_repo
            .get_assumption_by_id_for_simulation(uuid, sim.id)
            .await
        {
            Ok(a) => a,
            Err(_) => {
                tracing::warn!(
                    requested_assumption = %uuid,
                    simulation_id = %sim.id,
                    user_id = %user_id,
                    "calibrate_assumptions: rejected missing or out-of-scope assumption_id"
                );
                skipped_ids.push(id_str.clone());
                continue;
            }
        };
        let Some(field) = a.profile_field.as_deref().filter(|s| !s.is_empty()) else {
            skipped_ids.push(id_str.clone());
            continue;
        };
        let Some(val) = a.user_override_value.as_deref().filter(|s| !s.is_empty()) else {
            skipped_ids.push(id_str.clone());
            continue;
        };
        if apply_override_to_patch(&mut patch, field, val) {
            applied_ids.push(id_str.clone());
            applied_details.push((field.to_string(), val.to_string()));
            if !updated_fields.iter().any(|f| f == field) {
                updated_fields.push(field.to_string());
            }
        } else {
            skipped_ids.push(id_str.clone());
        }
    }

    // Generate AI summary describing the calibration; fall back on error.
    let ai_summary = build_calibration_summary(&state, &applied_details).await;

    // Apply patch + mark calibrated in a single tx.
    let mut tx = match state.pool.begin().await {
        Ok(t) => t,
        Err(e) => {
            tracing::error!(error = ?e, "calibrate_assumptions: begin tx");
            return write_error(StatusCode::INTERNAL_SERVER_ERROR, "begin tx");
        }
    };

    if patch.has_updates() {
        if let Err(e) = state
            .user_repo
            .apply_assumption_calibration_tx(&mut tx, user_id, &patch, &ai_summary, None)
            .await
        {
            tracing::error!(error = ?e, "calibrate_assumptions: apply patch");
            return write_error(StatusCode::INTERNAL_SERVER_ERROR, "apply calibration");
        }
    }

    if let Err(e) = state
        .simulation_repo
        .mark_assumptions_calibrated_tx(&mut tx, sim_id)
        .await
    {
        tracing::error!(error = ?e, "calibrate_assumptions: mark calibrated");
        return write_error(StatusCode::INTERNAL_SERVER_ERROR, "mark calibrated");
    }

    if let Err(e) = tx.commit().await {
        tracing::error!(error = ?e, "calibrate_assumptions: commit tx");
        return write_error(StatusCode::INTERNAL_SERVER_ERROR, "commit tx");
    }

    // Re-read sim to get the canonical calibrated_at timestamp for the response.
    let used_at = state
        .simulation_repo
        .get_simulation_by_id(sim_id)
        .await
        .ok()
        .and_then(|s| s.assumptions_calibrated_at);

    write_json(
        StatusCode::OK,
        json!({
            "status": "updated",
            "applied_assumption_ids": applied_ids,
            "skipped_assumption_ids": skipped_ids,
            "updated_fields": updated_fields,
            "summary": ai_summary,
            "assumptions_locked": true,
            "can_teach_vidente": false,
            "teach_vidente_used_at": used_at,
        }),
    )
}

/// Apply a single override value to the profile patch. Returns true if the field was
/// recognized and parsed successfully.
fn apply_override_to_patch(
    patch: &mut UserCalibrationProfilePatch,
    field: &str,
    val: &str,
) -> bool {
    let trimmed = val.trim();
    if trimmed.is_empty() {
        return false;
    }
    match field {
        "estimated_net_worth" => match trimmed.parse::<f64>() {
            Ok(v) => {
                patch.estimated_net_worth = Some(v);
                true
            }
            Err(_) => false,
        },
        "estimated_yearly_salary" => match trimmed.parse::<f64>() {
            Ok(v) => {
                patch.estimated_yearly_salary = Some(v);
                true
            }
            Err(_) => false,
        },
        "dependent_count" => match trimmed.parse::<i32>() {
            Ok(v) => {
                patch.dependent_count = Some(v);
                true
            }
            Err(_) => false,
        },
        "risk_tolerance" => {
            patch.risk_tolerance = Some(trimmed.to_string());
            true
        }
        "follow_through" => {
            patch.follow_through = Some(trimmed.to_string());
            true
        }
        "optimism_bias" => {
            patch.optimism_bias = Some(trimmed.to_string());
            true
        }
        "stress_response" => {
            patch.stress_response = Some(trimmed.to_string());
            true
        }
        "decision_style" => {
            patch.decision_style = Some(trimmed.to_string());
            true
        }
        "saving_habits" => {
            patch.saving_habits = Some(trimmed.to_string());
            true
        }
        "debt_comfort" => {
            patch.debt_comfort = Some(trimmed.to_string());
            true
        }
        "housing_stability" => {
            patch.housing_stability = Some(trimmed.to_string());
            true
        }
        "income_stability" => {
            patch.income_stability = Some(trimmed.to_string());
            true
        }
        "liquid_net_worth_source" => {
            patch.liquid_net_worth_source = Some(trimmed.to_string());
            true
        }
        "relationship_status" => {
            patch.relationship_status = Some(trimmed.to_string());
            true
        }
        "household_income_structure" => {
            patch.household_income_structure = Some(trimmed.to_string());
            true
        }
        "life_stability" => {
            patch.life_stability = Some(trimmed.to_string());
            true
        }
        _ => false,
    }
}

async fn build_calibration_summary(state: &AppState, applied: &[(String, String)]) -> String {
    if applied.is_empty() {
        return "No calibration changes applied.".to_string();
    }
    let bullets = applied
        .iter()
        .map(|(f, v)| format!("- {} → {}", f, v))
        .collect::<Vec<_>>()
        .join("\n");
    let fallback = format!("Calibration applied to:\n{}", bullets);
    let sys = "You are a concise profile-calibration assistant. Given a list of user_profiles field updates, write one or two sentences describing the calibration in natural language. No lists, no markdown. Under 60 words.".to_string();
    let user = format!("Applied field updates:\n{}\n\nWrite the summary.", bullets);
    match state
        .text_provider
        .generate_text(&TextRequest {
            system_prompt: sys,
            user_prompt: user,
            max_tokens: 200,
            temperature: 0.2,
            json_mode: false,
            ..Default::default()
        })
        .await
    {
        Ok(resp) if !resp.content.trim().is_empty() => resp.content.trim().to_string(),
        _ => fallback,
    }
}

#[derive(Deserialize, Default)]
pub struct ResimulateReq {
    #[serde(default)]
    pub edited_assumption_ids: Vec<String>,
}

pub async fn resimulate(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    Path(sim_id): Path<Uuid>,
    body: Option<Json<ResimulateReq>>,
) -> Response {
    let parent = match state.simulation_repo.get_simulation_by_id(sim_id).await {
        Ok(s) => s,
        Err(_) => return write_error(StatusCode::NOT_FOUND, "simulation not found"),
    };
    if parent.user_id != user_id {
        return write_error(StatusCode::NOT_FOUND, "simulation not found");
    }

    // Consume entitlement (if billing enabled).
    if state.billing.billing_enabled() {
        let mut tx = match state.pool.begin().await {
            Ok(t) => t,
            Err(e) => {
                tracing::error!(error = ?e, "resimulate: begin tx");
                return write_error(StatusCode::INTERNAL_SERVER_ERROR, "begin tx");
            }
        };
        if let Err(e) = state
            .subscription_repo
            .consume_cinematic_entitlement_tx(&mut tx, user_id)
            .await
        {
            return write_billing_error(&e.code, &e.message);
        }
        if let Err(e) = tx.commit().await {
            tracing::error!(error = ?e, "resimulate: commit tx");
            return write_error(StatusCode::INTERNAL_SERVER_ERROR, "commit tx");
        }
    }

    // Build assumption overrides JSON from edited assumption IDs.
    let overrides_json: JsonValue = {
        let edited_ids = body.map(|b| b.0.edited_assumption_ids).unwrap_or_default();
        let mut overrides: Vec<JsonValue> = Vec::new();
        for id in edited_ids {
            if let Ok(uuid) = Uuid::parse_str(&id) {
                if let Ok(a) = state
                    .simulation_repo
                    .get_assumption_by_id_for_simulation(uuid, parent.id)
                    .await
                {
                    overrides.push(json!({
                        "id": a.id,
                        "description": a.description,
                        "override_value": a.user_override_value,
                        "confidence": a.confidence,
                        "original_confidence": a.original_confidence,
                    }));
                } else {
                    tracing::warn!(
                        requested_assumption = %uuid,
                        parent_simulation_id = %parent.id,
                        user_id = %user_id,
                        "resimulate: rejected missing or out-of-scope assumption_id"
                    );
                }
            }
        }
        JsonValue::Array(overrides)
    };

    // Snapshot user context fresh.
    let profile = match state.user_repo.get_profile_by_user_id(user_id).await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(error = ?e, "resimulate: load profile");
            return write_error(StatusCode::INTERNAL_SERVER_ERROR, "load profile");
        }
    };
    let life_state = state
        .user_repo
        .build_life_state(user_id)
        .await
        .unwrap_or_else(|_| models::LifeState::default_state());
    let behavioral = crate::models::resolve_behavioral_profile(&profile);
    let financial_profile = crate::models::resolve_financial_profile(&profile);
    let life_context = crate::models::resolve_life_context_profile(&profile);
    let fact_sheet =
        crate::financial::build_financial_fact_sheet(&profile, &life_state, &financial_profile);
    let decision = match state
        .decision_repo
        .get_decision_by_id(parent.decision_id)
        .await
    {
        Ok(d) => d,
        Err(_) => {
            return write_error(StatusCode::NOT_FOUND, "parent decision not found");
        }
    };
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
        "parent_simulation_id": parent.id,
    });
    let ls_snap = serde_json::to_value(&life_state).unwrap_or(JsonValue::Null);

    let run_number = state
        .simulation_repo
        .get_max_run_number(parent.decision_id)
        .await
        .unwrap_or(0)
        + 1;

    let new_sim_id = Uuid::new_v4();
    let sim = models::DecisionSimulation {
        id: new_sim_id,
        decision_id: parent.decision_id,
        user_id,
        status: models::simulation_status::RUNNING.into(),
        total_components: 2,
        completed_components: 0,
        run_type: models::simulation_run_type::CINEMATIC.into(),
        user_context_snapshot: snapshot,
        life_state_snapshot: ls_snap,
        data_completeness: life_state.completeness,
        started_at: Some(chrono::Utc::now()),
        completed_at: None,
        created_at: chrono::Utc::now(),
        parent_simulation_id: Some(parent.id),
        run_number,
        assumption_overrides: overrides_json,
        assumptions_calibrated_at: None,
    };
    if let Err(e) = state.simulation_repo.create_simulation(&sim).await {
        tracing::error!(error = ?e, "resimulate: create simulation");
        return write_error(StatusCode::INTERNAL_SERVER_ERROR, "create simulation");
    }

    // Register components and enqueue the scenario planner.
    let components = vec![
        models::SimulationComponent {
            id: Uuid::nil(),
            simulation_id: new_sim_id,
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
            simulation_id: new_sim_id,
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
    if let Err(e) = state
        .components_repo
        .upsert_simulation_components(&components)
        .await
    {
        tracing::error!(error = ?e, "resimulate: register components");
        return write_error(StatusCode::INTERNAL_SERVER_ERROR, "register components");
    }

    if let Err(e) = state
        .job_client
        .insert(
            jobs::KIND_SCENARIO_PLANNER,
            &ScenarioPlannerArgs {
                simulation_id: new_sim_id,
            },
        )
        .await
    {
        tracing::error!(error = ?e, "resimulate: enqueue planner");
        return write_error(StatusCode::INTERNAL_SERVER_ERROR, "enqueue planner");
    }

    write_json(
        StatusCode::OK,
        json!({
            "simulation_id": new_sim_id,
            "run_type": models::simulation_run_type::CINEMATIC,
            "status": "running",
        }),
    )
}

pub async fn get_all_user_assumptions(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
) -> Response {
    let decisions = state
        .decision_repo
        .list_decisions_by_user_id(user_id)
        .await
        .unwrap_or_default();
    let mut all: Vec<JsonValue> = Vec::new();
    for d in decisions {
        let sim = match state
            .simulation_repo
            .get_simulation_by_decision_id(d.id)
            .await
        {
            Ok(s) => s,
            Err(_) => continue,
        };
        let assumptions = match state
            .simulation_repo
            .get_assumptions_by_simulation_id(sim.id)
            .await
        {
            Ok(a) => a,
            Err(_) => continue,
        };
        for a in assumptions {
            let mut obj = serde_json::to_value(&a).unwrap_or(JsonValue::Null);
            if let Some(map) = obj.as_object_mut() {
                map.insert(
                    "decision_text".into(),
                    JsonValue::String(d.decision_text.clone()),
                );
                map.insert("decision_id".into(), JsonValue::String(d.id.to_string()));
            }
            all.push(obj);
        }
    }
    write_json(StatusCode::OK, json!({ "assumptions": all }))
}

#[derive(Deserialize)]
pub struct ClarifyInsightReq {
    pub user_override_value: String,
}

pub async fn clarify_insight_assumption(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    Path(aid): Path<Uuid>,
    Json(req): Json<ClarifyInsightReq>,
) -> Response {
    let override_val = req.user_override_value.trim();
    if override_val.is_empty() {
        return write_error(StatusCode::BAD_REQUEST, "user_override_value is required");
    }
    match state
        .simulation_repo
        .update_assumption_for_user(aid, user_id, Some(override_val), None)
        .await
    {
        Ok(true) => write_json(StatusCode::OK, json!({"status": "ok"})),
        Ok(false) => write_error(StatusCode::NOT_FOUND, "assumption not found"),
        Err(e) => {
            tracing::error!(error = ?e, "clarify_insight_assumption: failed to clarify assumption");
            write_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to clarify assumption",
            )
        }
    }
}

pub async fn get_decision_simulations(
    State(state): State<AppState>,
    Extension(_): Extension<AuthUser>,
    Path(decision_id): Path<Uuid>,
) -> Response {
    let list = state
        .simulation_repo
        .list_simulation_versions(decision_id)
        .await
        .unwrap_or_default();
    write_json(StatusCode::OK, json!({ "simulations": list }))
}
