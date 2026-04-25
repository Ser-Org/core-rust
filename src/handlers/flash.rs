use crate::app_state::AppState;
use crate::billing::BillingError;
use crate::handlers::{write_billing_error, write_error, write_json};
use crate::jobs::{self, FlashGenerationArgs};
use crate::middleware::AuthUser;
use crate::models::{self, FlashVision};
use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::Response,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct PostFlashReq {
    pub question: String,
    #[serde(default)]
    pub input_method: String,
}

pub async fn post_flash(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    Json(req): Json<PostFlashReq>,
) -> Response {
    if req.question.is_empty() {
        return write_error(StatusCode::BAD_REQUEST, "question is required");
    }
    let method = if req.input_method.is_empty() {
        "text".into()
    } else {
        req.input_method
    };

    // Consume flash entitlement in a transaction. Prod-only: dev/staging skip
    // the check so testing never trips the what-if limit, even if Stripe is
    // configured for those environments.
    if state.billing.billing_enabled() && !state.cfg.is_development() {
        let mut tx = match state.pool.begin().await {
            Ok(t) => t,
            Err(e) => {
                tracing::error!(error = ?e, "post_flash: begin tx");
                return write_error(StatusCode::INTERNAL_SERVER_ERROR, "begin tx");
            }
        };
        match state
            .subscription_repo
            .consume_flash_entitlement_tx(&mut tx, user_id)
            .await
        {
            Ok(_) => {
                if let Err(e) = tx.commit().await {
                    tracing::error!(error = ?e, "post_flash: commit tx");
                    return write_error(StatusCode::INTERNAL_SERVER_ERROR, "commit tx");
                }
            }
            Err(e) => {
                return write_billing_error(&e.code, &e.message);
            }
        }
    }

    let vision = FlashVision {
        id: Uuid::new_v4(),
        user_id,
        question: req.question.clone(),
        input_method: method,
        status: models::flash_status::PENDING.into(),
        photo_url: None,
        music_url: None,
        error_message: None,
        share_token: None,
        is_public: false,
        completed_at: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    if let Err(e) = state.flash_repo.create_flash_vision(&vision).await {
        tracing::error!(error = ?e, "post_flash: failed to create flash vision");
        return write_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to create flash vision",
        );
    }
    let _ = state
        .job_client
        .insert(
            jobs::KIND_FLASH_GENERATION,
            &FlashGenerationArgs {
                flash_vision_id: vision.id,
                user_id,
            },
        )
        .await;
    write_json(StatusCode::CREATED, json!({ "flash_id": vision.id }))
}

#[derive(Deserialize)]
pub struct FlashListQuery {
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
}

pub async fn list_flash(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    Query(q): Query<FlashListQuery>,
) -> Response {
    let limit = q.limit.unwrap_or(20).clamp(1, 100);
    let offset = q.offset.unwrap_or(0).max(0);
    match state
        .flash_repo
        .list_flash_visions_by_user(user_id, limit, offset)
        .await
    {
        Ok((items, total)) => {
            let flashes: Vec<serde_json::Value> = items
                .into_iter()
                .map(|(vision, cover)| {
                    let mut obj = serde_json::to_value(vision).unwrap_or(json!({}));
                    if let Some(url) = cover {
                        obj["cover_image_url"] = serde_json::Value::String(url);
                    }
                    obj
                })
                .collect();
            write_json(
                StatusCode::OK,
                json!({ "flashes": flashes, "total": total, "limit": limit, "offset": offset }),
            )
        }
        Err(e) => {
            tracing::error!(error = ?e, "list_flash: failed to list flashes");
            write_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to list flashes")
        }
    }
}

pub async fn get_flash(
    State(state): State<AppState>,
    Extension(_): Extension<AuthUser>,
    Path(flash_id): Path<Uuid>,
) -> Response {
    let vision = match state.flash_repo.get_flash_vision_by_id(flash_id).await {
        Ok(v) => v,
        Err(_) => return write_error(StatusCode::NOT_FOUND, "flash not found"),
    };
    let images = state
        .flash_repo
        .get_flash_images_by_vision_id(vision.id)
        .await
        .unwrap_or_default();
    let mut enriched = Vec::with_capacity(images.len());
    for img in images {
        let signed = state
            .object_store
            .get_signed_url(
                &state.cfg.flash_images_bucket,
                &img.storage_path,
                Duration::from_secs(24 * 3600),
            )
            .await
            .unwrap_or(img.storage_url.clone());
        enriched.push(json!({
            "index": img.index,
            "url": signed,
            "prompt_used": img.prompt_used,
        }));
    }
    let music_url = vision.music_url.clone().unwrap_or_default();
    write_json(
        StatusCode::OK,
        json!({
            "flash_id": vision.id,
            "question": vision.question,
            "status": vision.status,
            "images": enriched,
            "music_url": music_url,
            "created_at": vision.created_at,
        }),
    )
}

pub async fn get_flash_status(
    State(state): State<AppState>,
    Extension(_): Extension<AuthUser>,
    Path(flash_id): Path<Uuid>,
) -> Response {
    let vision = match state.flash_repo.get_flash_vision_by_id(flash_id).await {
        Ok(v) => v,
        Err(_) => return write_error(StatusCode::NOT_FOUND, "flash not found"),
    };
    let count = state
        .flash_repo
        .count_completed_images(flash_id)
        .await
        .unwrap_or(0);
    write_json(
        StatusCode::OK,
        json!({
            "flash_id": vision.id,
            "status": vision.status,
            "completed_images": count,
            "total_images": 6,
            "completed_at": vision.completed_at,
        }),
    )
}

pub async fn post_flash_share(
    State(state): State<AppState>,
    Extension(_): Extension<AuthUser>,
    Path(flash_id): Path<Uuid>,
) -> Response {
    match state.flash_repo.set_share_token(flash_id).await {
        Ok(token) => write_json(
            StatusCode::OK,
            json!({ "share_token": token, "share_url": format!("{}/flash/shared/{}", state.cfg.app_url, token) }),
        ),
        Err(e) => {
            tracing::error!(error = ?e, "post_flash_share: failed to create share link");
            write_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to create share link",
            )
        }
    }
}

pub async fn check_flash_entitlement(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
) -> Response {
    // Mirror the gate on post_flash: dev/staging always report entitled.
    if !state.billing.billing_enabled() || state.cfg.is_development() {
        return write_json(StatusCode::OK, json!({"entitled": true}));
    }
    match state
        .subscription_repo
        .check_flash_entitlement(user_id)
        .await
    {
        Ok(()) => write_json(StatusCode::OK, json!({"entitled": true})),
        Err(e) => write_billing_error(&e.code, &e.message),
    }
}
