use crate::app_state::AppState;
use crate::handlers::{write_error, write_json};
use crate::middleware::AuthUser;
use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    response::Response,
};
use serde_json::json;
use std::time::Duration;
use uuid::Uuid;

pub async fn get_media(
    State(state): State<AppState>,
    Extension(_): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Response {
    let m = match state.media_repo.get_media_by_id(id).await {
        Ok(m) => m,
        Err(_) => return write_error(StatusCode::NOT_FOUND, "media not found"),
    };
    let signed = state
        .object_store
        .get_signed_url(&state.cfg.s3_bucket, &m.storage_path, Duration::from_secs(3600))
        .await
        .unwrap_or(m.storage_url.clone());
    write_json(
        StatusCode::OK,
        json!({
            "id": m.id,
            "simulation_id": m.simulation_id,
            "media_type": m.media_type,
            "storage_url": signed,
            "storage_path": m.storage_path,
            "clip_role": m.clip_role,
            "clip_order": m.clip_order,
            "scenario_path": m.scenario_path,
            "scenario_phase": m.scenario_phase,
            "created_at": m.created_at,
        }),
    )
}
