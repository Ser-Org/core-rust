use crate::app_state::AppState;
use crate::handlers::{write_error, write_json};
use crate::middleware::AuthUser;
use crate::models;
use crate::repos::CinematicContextInput as _CinCtx;
use axum::{
    extract::{Extension, State},
    http::StatusCode,
    response::Response,
    Json,
};
use serde::Deserialize;
use serde_json::json;

pub async fn get_status(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
) -> Response {
    match state.user_repo.get_cinematic_context_status(user_id).await {
        Ok(completed) => write_json(
            StatusCode::OK,
            json!({"cinematic_context_completed": completed}),
        ),
        Err(e) => {
            tracing::error!(error = ?e, "get_status: failed to get status");
            write_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to get status")
        }
    }
}

#[derive(Deserialize)]
pub struct CinematicContextReq {
    #[serde(default)]
    pub age_bracket: String,
    #[serde(default)]
    pub gender: String,
    pub relationship_status: String,
    pub dependent_count: i32,
    pub living_situation: String,
    pub industry: String,
    pub career_stage: String,
    pub net_worth_bracket: String,
    pub income_bracket: String,
}

pub async fn post(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    Json(req): Json<CinematicContextReq>,
) -> Response {
    // Validate.
    if !req.age_bracket.is_empty() && !models::age_bracket::is_valid(&req.age_bracket) {
        return write_error(StatusCode::BAD_REQUEST, "invalid age_bracket");
    }
    if !req.gender.is_empty() && !models::gender::is_valid(&req.gender) {
        return write_error(StatusCode::BAD_REQUEST, "invalid gender");
    }
    if !models::living_situation::is_valid(&req.living_situation) {
        return write_error(StatusCode::BAD_REQUEST, "invalid living_situation");
    }
    if !models::net_worth_bracket::is_valid(&req.net_worth_bracket) {
        return write_error(StatusCode::BAD_REQUEST, "invalid net_worth_bracket");
    }
    if !models::income_bracket::is_valid(&req.income_bracket) {
        return write_error(StatusCode::BAD_REQUEST, "invalid income_bracket");
    }
    if !models::career_stage::is_valid(&req.career_stage) {
        return write_error(StatusCode::BAD_REQUEST, "invalid career_stage");
    }
    if !models::industry::is_valid(&req.industry) {
        return write_error(StatusCode::BAD_REQUEST, "invalid industry");
    }

    let input = _CinCtx {
        age_bracket: req.age_bracket,
        gender: req.gender,
        relationship_status: req.relationship_status,
        dependent_count: req.dependent_count,
        living_situation: req.living_situation,
        industry: req.industry,
        career_stage: req.career_stage,
        net_worth_bracket: req.net_worth_bracket,
        income_bracket: req.income_bracket,
    };
    if let Err(e) = state.user_repo.ensure_profile(user_id).await {
        tracing::error!(error = ?e, "post: failed to save");
        return write_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to save");
    }
    if let Err(e) = state
        .user_repo
        .upsert_cinematic_context(user_id, &input)
        .await
    {
        tracing::error!(error = ?e, "post: failed to save");
        return write_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to save");
    }
    write_json(StatusCode::OK, json!({"status": "ok"}))
}
