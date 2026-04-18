pub mod billing;
pub mod cinematic_context;
pub mod decisions;
pub mod flash;
pub mod media;
pub mod onboarding;
pub mod scenario;
pub mod simulations;

use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;
use serde_json::json;

pub fn write_error(status: StatusCode, msg: &str) -> axum::response::Response {
    (status, Json(json!({ "error": msg }))).into_response()
}

pub fn write_json<T: Serialize>(status: StatusCode, value: T) -> axum::response::Response {
    (status, Json(value)).into_response()
}

pub fn write_billing_error(code: &str, message: &str) -> axum::response::Response {
    (
        StatusCode::PAYMENT_REQUIRED,
        Json(json!({ "error": message, "code": code })),
    )
        .into_response()
}
