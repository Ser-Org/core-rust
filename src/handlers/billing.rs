use crate::app_state::AppState;
use crate::billing::BillingError;
use crate::handlers::{write_billing_error, write_error, write_json};
use crate::middleware::AuthUser;
use axum::{
    body::Bytes,
    extract::{Extension, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;

pub async fn get_subscription(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
) -> Response {
    match state.billing.get_subscription(user_id).await {
        Ok(sub) => write_json(StatusCode::OK, sub),
        Err(e) => {
            tracing::error!(error = ?e, "get_subscription: failed to load subscription");
            write_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load subscription",
            )
        }
    }
}

pub async fn check_cinematic_entitlement(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
) -> Response {
    match state.billing.check_cinematic_entitlement(user_id).await {
        Ok(()) => write_json(StatusCode::OK, json!({"entitled": true})),
        Err(BillingError::Entitlement { code, message }) => write_billing_error(&code, &message),
        Err(e) => {
            tracing::error!(error = ?e, "check_cinematic_entitlement: entitlement check failed");
            write_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "entitlement check failed",
            )
        }
    }
}

#[derive(Deserialize)]
pub struct CheckoutReq {
    pub plan: String,
}

pub async fn create_checkout_session(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    Json(req): Json<CheckoutReq>,
) -> Response {
    match state
        .billing
        .create_checkout_session(user_id, &req.plan)
        .await
    {
        Ok(url) => write_json(StatusCode::OK, json!({"url": url})),
        Err(BillingError::NotConfigured) => {
            write_error(StatusCode::SERVICE_UNAVAILABLE, "billing not configured")
        }
        Err(BillingError::InvalidPlan) => write_error(StatusCode::BAD_REQUEST, "invalid plan"),
        Err(e) => {
            tracing::error!(error = ?e, "create_checkout_session: failed to create checkout");
            write_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to create checkout",
            )
        }
    }
}

pub async fn create_extra_cinematic_checkout_session(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
) -> Response {
    match state
        .billing
        .create_extra_cinematic_checkout_session(user_id)
        .await
    {
        Ok(url) => write_json(StatusCode::OK, json!({"url": url})),
        Err(BillingError::NotConfigured) => {
            write_error(StatusCode::SERVICE_UNAVAILABLE, "billing not configured")
        }
        Err(BillingError::PaidPlanRequired) => {
            write_error(StatusCode::PAYMENT_REQUIRED, "active paid plan required")
        }
        Err(e) => {
            tracing::error!(error = ?e, "create_extra_cinematic_checkout_session: failed to create checkout");
            write_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to create checkout",
            )
        }
    }
}

pub async fn create_extra_whatif_checkout_session(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
) -> Response {
    match state
        .billing
        .create_extra_whatif_checkout_session(user_id)
        .await
    {
        Ok(url) => write_json(StatusCode::OK, json!({"url": url})),
        Err(BillingError::NotConfigured) => {
            write_error(StatusCode::SERVICE_UNAVAILABLE, "billing not configured")
        }
        Err(BillingError::PaidPlanRequired) => {
            write_error(StatusCode::PAYMENT_REQUIRED, "active paid plan required")
        }
        Err(e) => {
            tracing::error!(error = ?e, "create_extra_whatif_checkout_session: failed to create checkout");
            write_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to create checkout",
            )
        }
    }
}

pub async fn create_whatif_10pack_checkout_session(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
) -> Response {
    match state
        .billing
        .create_whatif_10pack_checkout_session(user_id)
        .await
    {
        Ok(url) => write_json(StatusCode::OK, json!({"url": url})),
        Err(BillingError::NotConfigured) => {
            write_error(StatusCode::SERVICE_UNAVAILABLE, "billing not configured")
        }
        Err(BillingError::PaidPlanRequired) => {
            write_error(StatusCode::PAYMENT_REQUIRED, "active paid plan required")
        }
        Err(e) => {
            tracing::error!(error = ?e, "create_whatif_10pack_checkout_session: failed to create checkout");
            write_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to create checkout",
            )
        }
    }
}

pub async fn create_portal_session(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
) -> Response {
    match state.billing.create_portal_session(user_id).await {
        Ok(url) => write_json(StatusCode::OK, json!({"url": url})),
        Err(BillingError::NotConfigured) => {
            write_error(StatusCode::SERVICE_UNAVAILABLE, "billing not configured")
        }
        Err(BillingError::NoStripeCustomer) => {
            write_error(StatusCode::BAD_REQUEST, "no stripe customer on file")
        }
        Err(e) => {
            tracing::error!(error = ?e, "create_portal_session: failed to create portal session");
            write_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to create portal session",
            )
        }
    }
}

pub async fn stripe_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let sig = headers
        .get("Stripe-Signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    match state.billing.handle_webhook(&body, sig).await {
        Ok(()) => (StatusCode::OK, "ok").into_response(),
        Err(BillingError::NotConfigured) => {
            (StatusCode::SERVICE_UNAVAILABLE, "not configured").into_response()
        }
        Err(e) => {
            tracing::error!(error = ?e, "stripe_webhook: stripe webhook");
            (StatusCode::BAD_REQUEST, format!("{}", e)).into_response()
        }
    }
}
