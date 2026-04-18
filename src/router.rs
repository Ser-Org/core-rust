use crate::app_state::AppState;
use crate::handlers::*;
use crate::middleware::{
    auth_middleware, request_id_middleware, request_logger_middleware, AuthConfig,
};
use axum::{
    http::{HeaderValue, Method, StatusCode},
    response::IntoResponse,
    routing::{get, patch, post},
    Json, Router,
};
use serde_json::json;
use tower_http::cors::{Any, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;

pub fn build_router(state: AppState, auth_cfg: AuthConfig) -> Router {
    let cors = CorsLayer::new()
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers(Any)
        .allow_origin(Any);

    // Public routes.
    let public = Router::new()
        .route("/api/v1/health", get(health))
        .route(
            "/api/v1/stripe/webhook",
            post(billing::stripe_webhook),
        )
        .with_state(state.clone());

    // Authenticated routes.
    let authed = Router::new()
        // Onboarding
        .route("/api/v1/onboarding/path", post(onboarding::post_onboarding_path))
        .route("/api/v1/onboarding/life-story", post(onboarding::post_life_story))
        .route("/api/v1/onboarding/questions", post(onboarding::post_questions))
        .route("/api/v1/onboarding/identity", post(onboarding::post_identity))
        .route("/api/v1/onboarding/routines/infer", post(onboarding::post_infer_routines))
        .route("/api/v1/onboarding/routines/confirm", post(onboarding::post_routines_confirm))
        .route("/api/v1/onboarding/photo", post(onboarding::post_photo))
        .route("/api/v1/onboarding/complete", post(onboarding::post_complete))
        .route("/api/v1/onboarding/suggested-decision", get(onboarding::get_suggested_decision))
        .route("/api/v1/onboarding/suggested-what-if", get(onboarding::get_suggested_what_if))
        // Cinematic context gate
        .route("/api/v1/users/cinematic-context/status", get(cinematic_context::get_status))
        .route("/api/v1/users/cinematic-context", post(cinematic_context::post))
        // Profile
        .route("/api/v1/profile", get(onboarding::get_profile).patch(onboarding::patch_profile))
        .route("/api/v1/life-context", patch(onboarding::patch_life_context))
        .route("/api/v1/life-state", get(onboarding::get_life_state))
        // Decisions
        .route("/api/v1/decisions", post(decisions::post_decision).get(decisions::list_decisions))
        .route("/api/v1/decisions/:id", get(decisions::get_decision))
        .route("/api/v1/decisions/:id/answers", post(decisions::post_answers))
        // Simulations
        .route("/api/v1/decisions/:id/simulation/status", get(simulations::get_simulation_status))
        .route("/api/v1/decisions/:id/scenario", get(scenario::get_scenario_plan))
        .route("/api/v1/decisions/:id/media", get(scenario::get_simulation_media))
        .route("/api/v1/decisions/:id/simulation/progress", get(scenario::get_simulation_progress))
        .route("/api/v1/decisions/:id/simulations", get(scenario::get_decision_simulations))
        // Assumptions
        .route("/api/v1/simulations/:sim_id/assumptions", get(scenario::get_assumptions))
        .route("/api/v1/simulations/:sim_id/assumptions/:aid", patch(scenario::update_assumption))
        .route("/api/v1/simulations/:sim_id/calibrate", post(scenario::calibrate_assumptions))
        .route("/api/v1/simulations/:sim_id/resimulate", post(scenario::resimulate))
        .route("/api/v1/insights/assumptions", get(scenario::get_all_user_assumptions))
        .route("/api/v1/insights/assumptions/:aid/clarify", post(scenario::clarify_insight_assumption))
        // Media
        .route("/api/v1/media/:id", get(media::get_media))
        // Billing
        .route("/api/v1/billing/subscription", get(billing::get_subscription))
        .route("/api/v1/billing/check-cinematic", get(billing::check_cinematic_entitlement))
        .route("/api/v1/billing/checkout", post(billing::create_checkout_session))
        .route(
            "/api/v1/billing/checkout/extra-cinematic",
            post(billing::create_extra_cinematic_checkout_session),
        )
        .route("/api/v1/billing/portal", post(billing::create_portal_session))
        // Flash
        .route("/api/v1/flash", post(flash::post_flash).get(flash::list_flash))
        .route("/api/v1/flash/:flash_id", get(flash::get_flash))
        .route("/api/v1/flash/:flash_id/status", get(flash::get_flash_status))
        .route("/api/v1/flash/:flash_id/share", post(flash::post_flash_share))
        .route("/api/v1/billing/check-flash", get(flash::check_flash_entitlement))
        .route_layer(axum::middleware::from_fn_with_state(auth_cfg.clone(), auth_middleware))
        .with_state(state);

    // Layer order (outermost first): cors → request_id → request_logger →
    // (per-route auth) → handler. Applying layers here stacks them inside-out,
    // so `cors` ends up outermost.
    public
        .merge(authed)
        .layer(RequestBodyLimitLayer::new(25 * 1024 * 1024))
        .layer(axum::middleware::from_fn(request_logger_middleware))
        .layer(axum::middleware::from_fn(request_id_middleware))
        .layer(cors)
        .fallback(not_found)
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(json!({"status": "ok"})))
}

async fn not_found() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, Json(json!({"error": "not found"})))
}
