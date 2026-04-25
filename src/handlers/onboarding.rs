use crate::app_state::AppState;
use crate::handlers::{write_error, write_json};
use crate::jobs::worker::ensure_character_palette_jobs;
use crate::jobs::{self, LifeStateExtractionArgs};
use crate::media;
use crate::middleware::AuthUser;
use crate::models::{self, LifeStory, UserPhoto};
use crate::prompts::{self, SimulationContext};
use crate::providers::TextRequest;
use axum::{
    extract::{Extension, Multipart, State},
    http::StatusCode,
    response::Response,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use std::time::Duration;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct PathReq {
    pub path: String,
}

pub async fn post_onboarding_path(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    Json(req): Json<PathReq>,
) -> Response {
    if req.path != "story" && req.path != "questions" {
        tracing::debug!(user_id = %user_id, value = %req.path, "onboarding.path: invalid value");
        return write_error(
            StatusCode::BAD_REQUEST,
            "path must be one of: story, questions",
        );
    }
    tracing::info!(user_id = %user_id, path = %req.path, "onboarding.path: setting");
    if let Err(e) = state.user_repo.ensure_profile(user_id).await {
        tracing::error!(user_id = %user_id, error = ?e, "onboarding.path: failed to ensure profile");
        return write_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to ensure profile",
        );
    }
    if let Err(e) = state
        .user_repo
        .set_onboarding_path(user_id, &req.path)
        .await
    {
        tracing::error!(user_id = %user_id, error = ?e, "onboarding.path: failed to set onboarding path");
        return write_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to set onboarding path",
        );
    }
    tracing::info!(user_id = %user_id, path = %req.path, "onboarding.path: set");
    write_json(StatusCode::OK, json!({"status": "ok"}))
}

#[derive(Deserialize)]
pub struct LifeStoryReq {
    pub raw_input: String,
    #[serde(default)]
    pub input_method: String,
}

pub async fn post_life_story(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    Json(req): Json<LifeStoryReq>,
) -> Response {
    if req.raw_input.is_empty() {
        tracing::debug!(user_id = %user_id, "onboarding.life-story: missing raw_input");
        return write_error(StatusCode::BAD_REQUEST, "raw_input is required");
    }
    let method = if req.input_method.is_empty() {
        "text".into()
    } else {
        req.input_method
    };
    tracing::info!(
        user_id = %user_id,
        input_method = %method,
        input_length = req.raw_input.len(),
        "onboarding.life-story: processing"
    );

    let story = LifeStory {
        id: Uuid::new_v4(),
        user_id,
        raw_input: req.raw_input.clone(),
        input_method: method,
        ai_summary: String::new(),
        extracted_context: JsonValue::Null,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    let persisted_id = match state.user_repo.upsert_life_story(&story).await {
        Ok(id) => id,
        Err(e) => {
            tracing::error!(error = ?e, "post_life_story: failed to save life story");
            return write_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to save life story",
            );
        }
    };

    if let Err(e) = state.user_repo.ensure_profile(user_id).await {
        tracing::error!(error = ?e, "post_life_story: post life story");
    }
    if let Err(e) = state
        .user_repo
        .update_onboarding_status(user_id, models::onboarding_status::STORY_SUBMITTED)
        .await
    {
        tracing::error!(error = ?e, "post_life_story: post life story");
    }

    let _ = state
        .job_client
        .insert(
            jobs::KIND_LIFE_STATE_EXTRACTION,
            &LifeStateExtractionArgs {
                user_id,
                story_id: persisted_id,
            },
        )
        .await;

    tracing::info!(
        user_id = %user_id,
        story_id = %persisted_id,
        "onboarding.life-story: persisted"
    );
    write_json(StatusCode::OK, json!({"life_story_id": persisted_id}))
}

#[derive(Deserialize)]
pub struct IdentityReq {
    pub age_bracket: String,
    pub gender: String,
}

pub async fn post_identity(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    Json(req): Json<IdentityReq>,
) -> Response {
    if !models::age_bracket::is_valid(&req.age_bracket) {
        tracing::debug!(user_id = %user_id, value = %req.age_bracket, "onboarding.identity: invalid age_bracket");
        return write_error(StatusCode::BAD_REQUEST, "invalid age_bracket");
    }
    if !models::gender::is_valid(&req.gender) {
        tracing::debug!(user_id = %user_id, value = %req.gender, "onboarding.identity: invalid gender");
        return write_error(StatusCode::BAD_REQUEST, "invalid gender");
    }
    tracing::info!(
        user_id = %user_id,
        age_bracket = %req.age_bracket,
        gender = %req.gender,
        "onboarding.identity: saving"
    );
    if let Err(e) = state.user_repo.ensure_profile(user_id).await {
        tracing::error!(error = ?e, "post_identity: failed to save identity");
        return write_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to save identity");
    }
    if let Err(e) = state
        .user_repo
        .upsert_identity(user_id, &req.age_bracket, &req.gender)
        .await
    {
        tracing::error!(user_id = %user_id, error = ?e, "onboarding.identity: failed to save identity");
        return write_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to save identity");
    }
    tracing::info!(user_id = %user_id, "onboarding.identity: saved");
    write_json(StatusCode::OK, json!({"status": "ok"}))
}

pub async fn post_photo(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    mut multipart: Multipart,
) -> Response {
    tracing::info!(user_id = %user_id, "onboarding.photo: receiving upload");
    let mut data: Option<Vec<u8>> = None;
    let mut filename = String::new();
    let mut content_type = "image/jpeg".to_string();
    let mut photo_type = String::from(models::photo_type::FACE);

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if name == "photo" {
            filename = field.file_name().unwrap_or("").to_string();
            if let Some(ct) = field.content_type() {
                content_type = ct.to_string();
            }
            match field.bytes().await {
                Ok(b) => data = Some(b.to_vec()),
                Err(e) => {
                    tracing::error!(error = ?e, "post_photo: failed to read file");
                    return write_error(StatusCode::BAD_REQUEST, "failed to read file");
                }
            }
        } else if name == "photo_type" {
            if let Ok(t) = field.text().await {
                let t = t.trim().to_string();
                if !t.is_empty() {
                    photo_type = t;
                }
            }
        }
    }

    let data = match data {
        Some(d) => d,
        None => {
            tracing::debug!(user_id = %user_id, "onboarding.photo: missing file");
            return write_error(StatusCode::BAD_REQUEST, "photo file is required");
        }
    };
    if photo_type != models::photo_type::FACE && photo_type != models::photo_type::FULL_BODY {
        tracing::debug!(user_id = %user_id, value = %photo_type, "onboarding.photo: invalid photo_type");
        return write_error(
            StatusCode::BAD_REQUEST,
            "invalid photo_type (must be 'face' or 'full_body')",
        );
    }
    tracing::debug!(
        user_id = %user_id,
        filename = %filename,
        content_type = %content_type,
        size_bytes = data.len(),
        photo_type = %photo_type,
        "onboarding.photo: uploading to object store"
    );

    let ext = normalized_image_extension(&filename, &content_type);
    let path = format!("user-photos/{}/{}{}", user_id, Uuid::new_v4(), ext);
    let url = match state
        .object_store
        .upload(&state.cfg.s3_bucket, &path, data.clone(), &content_type)
        .await
    {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(user_id = %user_id, error = ?e, "onboarding.photo: failed to upload photo");
            return write_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to upload photo");
        }
    };

    let mut flux_url: Option<String> = None;
    let mut flux_path: Option<String> = None;
    if photo_type == models::photo_type::FACE {
        match media::resize_to_max_megapixels(&data, &content_type, 1.0, 90) {
            Ok((resized, _)) => {
                let fp = format!("user-photos/{}/{}-flux.jpg", user_id, Uuid::new_v4());
                match state
                    .object_store
                    .upload(&state.cfg.s3_bucket, &fp, resized.clone(), "image/jpeg")
                    .await
                {
                    Ok(u) => {
                        tracing::debug!(
                            storage_path = %fp,
                            size_bytes = resized.len(),
                            "onboarding.photo: flux-optimized derivative uploaded"
                        );
                        flux_url = Some(u);
                        flux_path = Some(fp);
                    }
                    Err(e) => {
                        tracing::warn!(error = ?e, "onboarding.photo: flux derivative upload failed, continuing with original")
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = ?e, "onboarding.photo: flux derivative resize failed, continuing with original")
            }
        }
    }

    let photo = UserPhoto {
        id: Uuid::new_v4(),
        user_id,
        storage_url: url.clone(),
        storage_path: path.clone(),
        mime_type: content_type.clone(),
        is_primary: true,
        photo_type: photo_type.clone(),
        created_at: chrono::Utc::now(),
        flux_storage_url: flux_url,
        flux_storage_path: flux_path,
    };
    if let Err(e) = state.user_repo.insert_user_photo(&photo).await {
        tracing::error!(user_id = %user_id, error = ?e, "onboarding.photo: failed to save photo record");
        return write_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to save photo record",
        );
    }
    tracing::info!(
        user_id = %user_id,
        photo_id = %photo.id,
        photo_type = %photo_type,
        "onboarding.photo: uploaded successfully"
    );
    write_json(
        StatusCode::OK,
        json!({"storage_url": url, "photo_type": photo_type}),
    )
}

fn normalized_image_extension(filename: &str, content_type: &str) -> String {
    let ext = filename
        .rsplit('.')
        .next()
        .map(|s| format!(".{}", s.to_lowercase()))
        .unwrap_or_default();
    match ext.as_str() {
        ".jpg" | ".jpeg" | ".png" | ".webp" => return ext,
        _ => {}
    }
    match content_type.to_lowercase().as_str() {
        "image/jpeg" | "image/jpg" => ".jpg".into(),
        "image/webp" => ".webp".into(),
        _ => ".png".into(),
    }
}

pub async fn post_complete(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
) -> Response {
    tracing::info!(user_id = %user_id, "onboarding.complete: finalizing onboarding");
    let photo_count = match state.user_repo.count_user_photos(user_id).await {
        Ok(n) => n,
        Err(e) => {
            tracing::error!(user_id = %user_id, error = ?e, "onboarding.complete: failed to verify photo upload");
            return write_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to verify photo upload",
            );
        }
    };
    if photo_count == 0 {
        tracing::warn!(user_id = %user_id, "onboarding.complete: blocked — no photos uploaded");
        return write_error(StatusCode::BAD_REQUEST, "at_least_one_photo_required");
    }
    if let Err(e) = state.user_repo.ensure_life_story(user_id).await {
        tracing::error!(user_id = %user_id, error = ?e, "onboarding.complete: failed to initialize life story");
        return write_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to initialize life story",
        );
    }

    // Generate dashboard snapshot synchronously (best-effort).
    match generate_dashboard_snapshot(&state, user_id).await {
        Ok(()) => {
            tracing::debug!(user_id = %user_id, "onboarding.complete: dashboard snapshot generated")
        }
        Err(e) => {
            tracing::warn!(user_id = %user_id, error = ?e, "onboarding.complete: dashboard snapshot failed (non-fatal)")
        }
    }

    let _ = state
        .user_repo
        .update_onboarding_status(user_id, models::onboarding_status::COMPLETE)
        .await;

    // Enqueue character palette generation (single-flight per user/photo).
    // The static baseline is the required identity anchor for first generated
    // experiences; dynamic variants are optional enhancements.
    match state.user_repo.get_primary_photo_by_user_id(user_id).await {
        Ok(photo) => {
            if let Err(e) =
                ensure_character_palette_jobs(&state, user_id, photo.id, "onboarding.complete")
                    .await
            {
                tracing::warn!(user_id = %user_id, error = ?e, "onboarding.complete: character palette generation setup failed (non-fatal)")
            }
        }
        Err(e) => {
            tracing::warn!(user_id = %user_id, error = ?e, "onboarding.complete: no primary photo for character plate (non-fatal)")
        }
    }

    // Non-blocking: suggested first decision + what-if
    let s_clone = state.clone();
    tokio::spawn(async move {
        match generate_suggested_first_decision(&s_clone, user_id).await {
            Ok(()) => {
                tracing::debug!(user_id = %user_id, "onboarding.complete: suggested first decision generated")
            }
            Err(e) => {
                tracing::warn!(user_id = %user_id, error = ?e, "onboarding.complete: suggested first decision failed (non-fatal)")
            }
        }
        match generate_suggested_first_what_if(&s_clone, user_id).await {
            Ok(()) => {
                tracing::debug!(user_id = %user_id, "onboarding.complete: suggested first what-if generated")
            }
            Err(e) => {
                tracing::warn!(user_id = %user_id, error = ?e, "onboarding.complete: suggested first what-if failed (non-fatal)")
            }
        }
    });

    tracing::info!(user_id = %user_id, "onboarding.complete: complete");
    write_json(StatusCode::OK, json!({"status": "complete"}))
}

async fn generate_dashboard_snapshot(state: &AppState, user_id: Uuid) -> anyhow::Result<()> {
    let profile = state.user_repo.get_profile_by_user_id(user_id).await?;
    let life_state = state
        .user_repo
        .build_life_state(user_id)
        .await
        .unwrap_or_else(|_| models::LifeState::default_state());
    let life_story = state
        .user_repo
        .get_life_story_by_user_id(user_id)
        .await
        .ok();
    let mut sctx = SimulationContext::default();
    sctx.user = Some(profile);
    sctx.life_state = life_state;
    sctx.life_story = life_story;
    let (sys, user) = state
        .prompt_builder
        .build_text_prompt(prompts::TASK_DASHBOARD, &sctx);
    let r = state
        .text_provider
        .generate_text(&TextRequest {
            system_prompt: sys,
            user_prompt: user,
            max_tokens: 4000,
            temperature: 0.5,
            json_mode: true,
            ..Default::default()
        })
        .await?;
    let parsed: JsonValue = serde_json::from_str(&r.content)?;
    state
        .simulation_repo
        .upsert_dashboard_snapshot(
            Uuid::new_v4(),
            user_id,
            &parsed
                .get("life_quality_trajectory")
                .cloned()
                .unwrap_or(JsonValue::Null),
            &parsed
                .get("life_momentum_score")
                .cloned()
                .unwrap_or(JsonValue::Null),
            &parsed
                .get("probability_outlook")
                .cloned()
                .unwrap_or(JsonValue::Null),
            parsed
                .get("narrative_summary")
                .and_then(|v| v.as_str())
                .unwrap_or(""),
            &r.content,
            chrono::Utc::now(),
        )
        .await?;
    Ok(())
}

async fn generate_suggested_first_decision(state: &AppState, user_id: Uuid) -> anyhow::Result<()> {
    let profile = state.user_repo.get_profile_by_user_id(user_id).await?;
    let mut sctx = SimulationContext::default();
    sctx.user = Some(profile);
    if let Ok(ls) = state.user_repo.build_life_state(user_id).await {
        sctx.life_state = ls;
    }
    let (sys, user) = prompts::suggested_first_decision(&sctx);
    let r = state
        .text_provider
        .generate_text(&TextRequest {
            system_prompt: sys,
            user_prompt: user,
            max_tokens: 1500,
            temperature: 0.5,
            json_mode: true,
            ..Default::default()
        })
        .await?;
    let parsed: JsonValue = serde_json::from_str(&r.content)?;
    state
        .user_repo
        .set_suggested_first_decision(user_id, &parsed)
        .await?;
    Ok(())
}

async fn generate_suggested_first_what_if(state: &AppState, user_id: Uuid) -> anyhow::Result<()> {
    let profile = state.user_repo.get_profile_by_user_id(user_id).await?;
    let mut sctx = SimulationContext::default();
    sctx.user = Some(profile);
    if let Ok(ls) = state.user_repo.build_life_state(user_id).await {
        sctx.life_state = ls;
    }
    let (sys, user) = prompts::suggested_first_what_if(&sctx);
    let r = state
        .text_provider
        .generate_text(&TextRequest {
            system_prompt: sys,
            user_prompt: user,
            max_tokens: 800,
            temperature: 0.6,
            json_mode: true,
            ..Default::default()
        })
        .await?;
    let parsed: JsonValue = serde_json::from_str(&r.content)?;
    state
        .user_repo
        .set_suggested_first_what_if(user_id, &parsed)
        .await?;
    Ok(())
}

pub async fn get_profile(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
) -> Response {
    let profile = match state.user_repo.get_profile_by_user_id(user_id).await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(error = ?e, "get_profile: failed to get profile");
            return write_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to get profile");
        }
    };
    let story = state
        .user_repo
        .get_life_story_by_user_id(user_id)
        .await
        .ok();

    let mut photo_url: Option<String> = None;
    if let Ok(photo) = state.user_repo.get_primary_photo_by_user_id(user_id).await {
        match state
            .object_store
            .get_signed_url(
                &state.cfg.s3_bucket,
                &photo.storage_path,
                Duration::from_secs(3600),
            )
            .await
        {
            Ok(s) => photo_url = Some(s),
            Err(_) => photo_url = Some(photo.storage_url.clone()),
        }
    }

    write_json(
        StatusCode::OK,
        json!({
            "estimated_net_worth": profile.estimated_net_worth,
            "estimated_yearly_salary": profile.estimated_yearly_salary,
            "raw_input": story.as_ref().map(|s| s.raw_input.clone()).unwrap_or_default(),
            "input_method": story.as_ref().map(|s| s.input_method.clone()).unwrap_or_default(),
            "onboarding_path": profile.onboarding_path,
            "onboarding_status": profile.onboarding_status,
            "photo_url": photo_url,
            "behavioral_profile": {
                "risk_tolerance": profile.risk_tolerance,
                "follow_through": profile.follow_through,
                "optimism_bias": profile.optimism_bias,
                "stress_response": profile.stress_response,
                "decision_style": profile.decision_style,
            },
            "financial_profile": {
                "saving_habits": profile.saving_habits,
                "debt_comfort": profile.debt_comfort,
                "housing_stability": profile.housing_stability,
                "income_stability": profile.income_stability,
            },
            "relationship_status": profile.relationship_status,
            "household_income_structure": profile.household_income_structure,
            "dependent_count": profile.dependent_count,
            "life_stability": profile.life_stability,
        }),
    )
}

#[derive(Deserialize)]
pub struct PatchProfileReq {
    #[serde(default)]
    pub estimated_net_worth: f64,
    #[serde(default)]
    pub estimated_yearly_salary: f64,
    pub risk_tolerance: Option<String>,
    pub follow_through: Option<String>,
    pub optimism_bias: Option<String>,
    pub stress_response: Option<String>,
    pub decision_style: Option<String>,
}

pub async fn patch_profile(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    Json(req): Json<PatchProfileReq>,
) -> Response {
    if let Err(e) = state
        .user_repo
        .update_financials(
            user_id,
            req.estimated_net_worth,
            req.estimated_yearly_salary,
        )
        .await
    {
        tracing::error!(error = ?e, "patch_profile: failed to update profile");
        return write_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to update profile",
        );
    }
    if req.risk_tolerance.is_some()
        || req.follow_through.is_some()
        || req.optimism_bias.is_some()
        || req.stress_response.is_some()
        || req.decision_style.is_some()
    {
        if let Err(e) = state
            .user_repo
            .update_behavioral_profile(
                user_id,
                req.risk_tolerance.as_deref(),
                req.follow_through.as_deref(),
                req.optimism_bias.as_deref(),
                req.stress_response.as_deref(),
                req.decision_style.as_deref(),
            )
            .await
        {
            tracing::error!(error = ?e, "patch_profile: failed to update profile");
            return write_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to update profile",
            );
        }
    }
    write_json(StatusCode::OK, json!({"status": "ok"}))
}

#[derive(Deserialize)]
pub struct LifeContextReq {
    pub raw_input: String,
    #[serde(default)]
    pub input_method: String,
}

pub async fn patch_life_context(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    Json(req): Json<LifeContextReq>,
) -> Response {
    if req.raw_input.is_empty() {
        return write_error(StatusCode::BAD_REQUEST, "raw_input is required");
    }
    let method = if req.input_method.is_empty() {
        "text".into()
    } else {
        req.input_method
    };
    let story = LifeStory {
        id: Uuid::new_v4(),
        user_id,
        raw_input: req.raw_input.clone(),
        input_method: method,
        ai_summary: String::new(),
        extracted_context: JsonValue::Null,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    let story_id = match state.user_repo.upsert_life_story(&story).await {
        Ok(id) => id,
        Err(e) => {
            tracing::error!(error = ?e, "patch_life_context: failed to update life context");
            return write_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to update life context",
            );
        }
    };
    let _ = state
        .job_client
        .insert(
            jobs::KIND_LIFE_STATE_EXTRACTION,
            &LifeStateExtractionArgs { user_id, story_id },
        )
        .await;
    write_json(StatusCode::OK, json!({"status": "ok"}))
}

pub async fn get_suggested_decision(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
) -> Response {
    match state.user_repo.get_suggested_first_decision(user_id).await {
        Ok(v) if !v.is_null() => write_json(StatusCode::OK, v),
        Ok(_) => write_json(StatusCode::ACCEPTED, json!({"status": "generating"})),
        Err(e) => {
            tracing::error!(error = ?e, "get_suggested_decision: failed to get suggested decision");
            write_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to get suggested decision",
            )
        }
    }
}

pub async fn get_suggested_what_if(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
) -> Response {
    match state.user_repo.get_suggested_first_what_if(user_id).await {
        Ok(v) if !v.is_null() => write_json(StatusCode::OK, v),
        Ok(_) => write_json(StatusCode::ACCEPTED, json!({"status": "generating"})),
        Err(e) => {
            tracing::error!(error = ?e, "get_suggested_what_if: failed to get suggested what-if");
            write_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to get suggested what-if",
            )
        }
    }
}

pub async fn get_life_state(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
) -> Response {
    match state.user_repo.build_life_state(user_id).await {
        Ok(ls) => write_json(StatusCode::OK, ls),
        Err(e) => {
            tracing::error!(error = ?e, "get_life_state: failed to load life state");
            write_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load life state",
            )
        }
    }
}
