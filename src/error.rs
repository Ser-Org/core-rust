use axum::{http::StatusCode, response::IntoResponse, Json};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("forbidden: {0}")]
    Forbidden(String),
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("payment required: {0}")]
    PaymentRequired(String),
    #[error("internal error: {0}")]
    Internal(String),
    #[error("sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("reqwest: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("anyhow: {0}")]
    Any(#[from] anyhow::Error),
}

impl AppError {
    pub fn internal<S: Into<String>>(msg: S) -> Self {
        Self::Internal(msg.into())
    }
    pub fn bad_request<S: Into<String>>(msg: S) -> Self {
        Self::BadRequest(msg.into())
    }
    pub fn not_found<S: Into<String>>(msg: S) -> Self {
        Self::NotFound(msg.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (status, msg) = match self {
            Self::Unauthorized(m) => (StatusCode::UNAUTHORIZED, m),
            Self::Forbidden(m) => (StatusCode::FORBIDDEN, m),
            Self::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            Self::NotFound(m) => (StatusCode::NOT_FOUND, m),
            Self::Conflict(m) => (StatusCode::CONFLICT, m),
            Self::PaymentRequired(m) => (StatusCode::PAYMENT_REQUIRED, m),
            Self::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, m),
            Self::Sqlx(e) => match e {
                sqlx::Error::RowNotFound => (StatusCode::NOT_FOUND, "not found".into()),
                _ => {
                    tracing::error!(error = ?e, "database error");
                    (StatusCode::INTERNAL_SERVER_ERROR, "database error".into())
                }
            },
            Self::Serde(e) => {
                tracing::error!(error = ?e, "serde error");
                (StatusCode::BAD_REQUEST, format!("invalid JSON: {}", e))
            }
            Self::Reqwest(e) => {
                tracing::error!(error = ?e, "http client error");
                (StatusCode::BAD_GATEWAY, "upstream error".into())
            }
            Self::Any(e) => {
                tracing::error!(error = ?e, "internal error");
                (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e))
            }
        };
        (status, Json(json!({ "error": msg }))).into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;
