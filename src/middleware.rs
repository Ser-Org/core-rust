//! HTTP middleware: request ID, structured request logging, and JWT auth.
//!
//! Every request gets a `request_id` (UUID) propagated as a header + tracing
//! span field. Auth middleware decodes the Supabase JWT and enriches the span
//! with `user_id` so every downstream log line carries both fields.

use axum::{
    body::Body,
    extract::Request,
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64URL, Engine as _};
use dashmap::DashMap;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;
use tracing::Instrument;
use uuid::Uuid;

// --- Auth config -----------------------------------------------------------

#[derive(Clone)]
pub struct AuthConfig {
    pub jwt_secret: String,
    pub jwks_url: String,
    pub jwks_cache: Arc<DashMap<String, Vec<u8>>>,
}

impl AuthConfig {
    pub async fn new(jwt_secret: String, jwks_url: String) -> Self {
        let cache = Arc::new(DashMap::new());
        let cfg = Self {
            jwt_secret,
            jwks_url: jwks_url.clone(),
            jwks_cache: cache.clone(),
        };
        if !jwks_url.is_empty() {
            match cfg.refresh_jwks().await {
                Ok(()) => {
                    tracing::info!(jwks_url = %jwks_url, keys = cache.len(), "auth: JWKS loaded")
                }
                Err(e) => {
                    tracing::warn!(jwks_url = %jwks_url, error = ?e, "auth: could not load JWKS at startup")
                }
            }
        }
        cfg
    }

    pub async fn refresh_jwks(&self) -> anyhow::Result<()> {
        if self.jwks_url.is_empty() {
            return Ok(());
        }
        #[derive(Deserialize)]
        struct Jwks {
            keys: Vec<Jwk>,
        }
        #[derive(Deserialize)]
        struct Jwk {
            kid: Option<String>,
            kty: String,
            #[serde(default)]
            crv: String,
            #[serde(default)]
            x: String,
            #[serde(default)]
            y: String,
            #[serde(default)]
            n: String,
            #[serde(default)]
            e: String,
        }
        let resp = reqwest::get(&self.jwks_url).await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("jwks: status {}", resp.status()));
        }
        let jwks: Jwks = resp.json().await?;
        for k in jwks.keys {
            let kid = match k.kid {
                Some(kid) => kid,
                None => continue,
            };
            match k.kty.as_str() {
                "EC" => {
                    if k.crv != "P-256" {
                        tracing::warn!(kid = %kid, crv = %k.crv, "jwks: skipping non-P-256 EC key");
                        continue;
                    }
                    let x = B64URL.decode(&k.x).unwrap_or_default();
                    let y = B64URL.decode(&k.y).unwrap_or_default();
                    if x.len() == 32 && y.len() == 32 {
                        let mut uncompressed = vec![0x04];
                        uncompressed.extend_from_slice(&x);
                        uncompressed.extend_from_slice(&y);
                        self.jwks_cache.insert(kid, uncompressed);
                    }
                }
                "RSA" => {
                    let key = format!("rsa:{}:{}", k.n, k.e);
                    self.jwks_cache.insert(kid, key.into_bytes());
                }
                other => {
                    tracing::debug!(kty = %other, kid = %kid, "jwks: skipping unsupported key type");
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct AuthUser(pub Uuid);

#[derive(Clone, Debug)]
pub struct RequestId(pub String);

// --- Request ID middleware -------------------------------------------------

pub async fn request_id_middleware(mut req: Request<Body>, next: Next) -> Response {
    let request_id = Uuid::new_v4().to_string();
    req.extensions_mut().insert(RequestId(request_id.clone()));
    let mut resp = next.run(req).await;
    if let Ok(hv) = axum::http::HeaderValue::from_str(&request_id) {
        resp.headers_mut().insert("X-Request-Id", hv);
    }
    resp
}

// --- Request logger middleware --------------------------------------------

/// Logs every HTTP request with method, path, status, and duration. Each
/// request runs inside a tracing span so downstream `tracing::info!` / etc.
/// calls automatically carry `request_id`, `method`, and `path` fields.
pub async fn request_logger_middleware(req: Request<Body>, next: Next) -> Response {
    let method = req.method().clone();
    let path = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| req.uri().path().to_string());
    let remote_addr = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .or_else(|| req.headers().get("x-real-ip").and_then(|v| v.to_str().ok()))
        .unwrap_or("")
        .to_string();
    let user_agent = req
        .headers()
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let request_id = req
        .extensions()
        .get::<RequestId>()
        .map(|r| r.0.clone())
        .unwrap_or_default();

    let span = tracing::info_span!(
        "http",
        request_id = %request_id,
        method = %method,
        path = %path,
    );

    let start = Instant::now();
    let short_path = truncate(&path, 120);
    tracing::debug!(remote_addr = %remote_addr, user_agent = %truncate(&user_agent, 120), "request started");

    let resp = next.run(req).instrument(span.clone()).await;
    let duration_ms = start.elapsed().as_millis() as u64;
    let status = resp.status().as_u16();

    let _enter = span.enter();
    if status >= 500 {
        tracing::error!(status, duration_ms, path = %short_path, "request completed");
    } else if status >= 400 {
        tracing::warn!(status, duration_ms, path = %short_path, "request completed");
    } else {
        tracing::info!(status, duration_ms, path = %short_path, "request completed");
    }
    resp
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

// --- Auth middleware -------------------------------------------------------

pub async fn auth_middleware(
    axum::extract::State(cfg): axum::extract::State<AuthConfig>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    let token = match extract_bearer(req.headers()) {
        Ok(t) => t,
        Err(resp) => {
            tracing::debug!("auth: missing or malformed authorization header");
            return resp;
        }
    };

    let header = match decode_header(&token) {
        Ok(h) => h,
        Err(e) => {
            tracing::debug!(error = ?e, "auth: invalid token header");
            return unauthorized("invalid token");
        }
    };

    let user_id = match header.alg {
        Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512 => {
            let key = DecodingKey::from_secret(cfg.jwt_secret.as_bytes());
            let mut validation = Validation::new(header.alg);
            validation.validate_exp = true;
            validation.validate_aud = false;
            match decode::<Claims>(&token, &key, &validation) {
                Ok(t) => match Uuid::parse_str(&t.claims.sub) {
                    Ok(u) => u,
                    Err(_) => {
                        tracing::debug!(sub = %t.claims.sub, "auth: invalid user ID in token");
                        return unauthorized("invalid user ID in token");
                    }
                },
                Err(e) => {
                    tracing::debug!(error = ?e, alg = ?header.alg, "auth: invalid token");
                    return unauthorized("invalid token");
                }
            }
        }
        Algorithm::ES256 => {
            let kid = match &header.kid {
                Some(k) => k.clone(),
                None => {
                    tracing::debug!("auth: ES256 token missing kid");
                    return unauthorized("missing kid");
                }
            };
            let mut pub_key_bytes = cfg.jwks_cache.get(&kid).map(|v| v.clone());
            if pub_key_bytes.is_none() {
                tracing::debug!(kid = %kid, "auth: unknown kid, refreshing JWKS");
                let _ = cfg.refresh_jwks().await;
                pub_key_bytes = cfg.jwks_cache.get(&kid).map(|v| v.clone());
            }
            let pub_key = match pub_key_bytes {
                Some(b) => b,
                None => {
                    tracing::debug!(kid = %kid, "auth: unknown key id after refresh");
                    return unauthorized("unknown kid");
                }
            };
            let key = if pub_key.len() >= 65 {
                match DecodingKey::from_ec_components(
                    &B64URL.encode(&pub_key[1..33]),
                    &B64URL.encode(&pub_key[33..65]),
                ) {
                    Ok(k) => k,
                    Err(e) => {
                        tracing::warn!(kid = %kid, error = ?e, "auth: invalid EC key material");
                        return unauthorized("invalid public key");
                    }
                }
            } else {
                match std::str::from_utf8(&pub_key) {
                    Ok(s) if s.starts_with("rsa:") => {
                        let parts: Vec<&str> = s.splitn(3, ':').collect();
                        if parts.len() == 3 {
                            match DecodingKey::from_rsa_components(parts[1], parts[2]) {
                                Ok(k) => k,
                                Err(e) => {
                                    tracing::warn!(kid = %kid, error = ?e, "auth: invalid RSA key material");
                                    return unauthorized("invalid public key");
                                }
                            }
                        } else {
                            tracing::warn!(kid = %kid, "auth: malformed RSA key cache entry");
                            return unauthorized("invalid public key");
                        }
                    }
                    _ => {
                        tracing::warn!(kid = %kid, "auth: unrecognized key material");
                        return unauthorized("invalid public key");
                    }
                }
            };
            let mut validation = Validation::new(header.alg);
            validation.validate_exp = true;
            validation.validate_aud = false;
            match decode::<Claims>(&token, &key, &validation) {
                Ok(t) => match Uuid::parse_str(&t.claims.sub) {
                    Ok(u) => u,
                    Err(_) => {
                        tracing::debug!(sub = %t.claims.sub, "auth: invalid user ID in token");
                        return unauthorized("invalid user ID in token");
                    }
                },
                Err(e) => {
                    tracing::debug!(error = ?e, "auth: invalid ES256 token");
                    return unauthorized("invalid token");
                }
            }
        }
        other => {
            tracing::warn!(alg = ?other, "auth: unsupported signing method");
            return unauthorized("unsupported signing method");
        }
    };

    // Attach to both extensions (for handler extraction) and the current span
    // (so every downstream log line carries user_id automatically).
    req.extensions_mut().insert(AuthUser(user_id));
    tracing::Span::current().record("user_id", tracing::field::display(user_id));
    tracing::trace!(user_id = %user_id, "auth: user authenticated");
    next.run(req).await
}

#[derive(Deserialize)]
struct Claims {
    sub: String,
    #[serde(default)]
    exp: i64,
}

fn extract_bearer(headers: &HeaderMap) -> Result<String, Response> {
    let auth = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| unauthorized("missing authorization header"))?;
    if let Some(rest) = auth.strip_prefix("Bearer ") {
        Ok(rest.to_string())
    } else {
        Err(unauthorized("invalid authorization format"))
    }
}

pub fn unauthorized(msg: &str) -> Response {
    (StatusCode::UNAUTHORIZED, Json(json!({"error": msg}))).into_response()
}
