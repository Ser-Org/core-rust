use crate::app_state::AppState;
use crate::handlers::{write_error, write_json};
use crate::middleware::AuthUser;
use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    response::Response,
};
use serde_json::json;
use uuid::Uuid;

pub async fn get_simulation_status(
    State(state): State<AppState>,
    Extension(AuthUser(_user_id)): Extension<AuthUser>,
    Path(decision_id): Path<Uuid>,
) -> Response {
    let sim = match state.simulation_repo.get_simulation_by_decision_id(decision_id).await {
        Ok(s) => s,
        Err(_) => {
            return write_json(
                StatusCode::OK,
                json!({
                    "status": "not_started",
                }),
            )
        }
    };
    write_json(
        StatusCode::OK,
        json!({
            "simulation_id": sim.id,
            "status": sim.status,
            "total_components": sim.total_components,
            "completed_components": sim.completed_components,
            "started_at": sim.started_at,
            "completed_at": sim.completed_at,
            "run_number": sim.run_number,
        }),
    )
}
