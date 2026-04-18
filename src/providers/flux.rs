use crate::providers::*;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const BASE_URL: &str = "https://api.bfl.ai/v1";
const MAX_ENDPOINT: &str = "/flux-2-max";
const POLL_INTERVAL: Duration = Duration::from_millis(500);
const POLL_TIMEOUT: Duration = Duration::from_secs(600); // 10min — was 90s; Flux occasionally runs long.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120); // was 30s.

pub struct FluxProvider {
    api_key: String,
    client: Client,
}

impl FluxProvider {
    pub fn new(api_key: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(300)) // 5min per HTTP request — was 120s.
            .build()
            .expect("reqwest client");
        Self { api_key, client }
    }
}

pub struct MockFluxProvider;
impl MockFluxProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl FlashImageProvider for MockFluxProvider {
    async fn generate_image(&self, _req: &FlashImageRequest) -> Result<FlashImageResponse> {
        Ok(FlashImageResponse {
            image_bytes: valid_placeholder_jpeg().to_vec(),
            mime_type: "image/jpeg".into(),
            seed_used: None,
            provider_name: "flux-mock".into(),
        })
    }
    async fn generate_image_with_input(
        &self,
        _req: &FlashImageWithInputRequest,
    ) -> Result<FlashImageResponse> {
        Ok(FlashImageResponse {
            image_bytes: valid_placeholder_jpeg().to_vec(),
            mime_type: "image/jpeg".into(),
            seed_used: None,
            provider_name: "flux-mock".into(),
        })
    }
}

/// 256×256 gray JPEG. Cached on first call. Size exists so downstream consumers
/// (e.g. Runway's ≥2×2 reference-image check) accept the mock output.
fn valid_placeholder_jpeg() -> &'static [u8] {
    static CACHE: OnceLock<Vec<u8>> = OnceLock::new();
    CACHE.get_or_init(|| {
        use image::{codecs::jpeg::JpegEncoder, ExtendedColorType, ImageEncoder};
        let w: u32 = 256;
        let h: u32 = 256;
        let pixels = vec![128u8; (w * h * 3) as usize];
        let mut out = Vec::new();
        JpegEncoder::new_with_quality(&mut out, 80)
            .write_image(&pixels, w, h, ExtendedColorType::Rgb8)
            .expect("mock jpeg encode");
        out
    })
}

#[derive(Serialize)]
struct SubmitRequest {
    prompt: String,
    #[serde(skip_serializing_if = "is_zero_u32")]
    width: u32,
    #[serde(skip_serializing_if = "is_zero_u32")]
    height: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    seed: Option<i64>,
    safety_tolerance: u32,
    #[serde(skip_serializing_if = "String::is_empty")]
    output_format: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    input_image: String,
}

fn is_zero_u32(v: &u32) -> bool { *v == 0 }

#[derive(Deserialize)]
struct SubmitResponse {
    #[serde(default)]
    id: String,
    polling_url: String,
}

#[derive(Deserialize, Default)]
struct PollResult {
    #[serde(default)]
    sample: String,
}

#[derive(Deserialize)]
struct PollResponse {
    status: String,
    #[serde(default)]
    result: PollResult,
}

#[async_trait]
impl FlashImageProvider for FluxProvider {
    async fn generate_image(&self, req: &FlashImageRequest) -> Result<FlashImageResponse> {
        let (width, height) = flux_safe_dims(req.width, req.height);
        let mut body = SubmitRequest {
            prompt: req.prompt.clone(),
            width,
            height,
            seed: req.seed,
            safety_tolerance: 2,
            output_format: req.output_format.clone(),
            input_image: String::new(),
        };
        if body.output_format.is_empty() {
            body.output_format = "jpeg".into();
        }
        let bytes = self.submit_and_poll(&body).await?;
        Ok(FlashImageResponse {
            image_bytes: bytes,
            mime_type: mime_for(&body.output_format),
            seed_used: req.seed,
            provider_name: "flux".into(),
        })
    }

    async fn generate_image_with_input(
        &self,
        req: &FlashImageWithInputRequest,
    ) -> Result<FlashImageResponse> {
        let input_base64 = self.normalize_input_image(&req.input_image).await?;
        let (width, height) = flux_safe_dims(req.width, req.height);
        let mut body = SubmitRequest {
            prompt: req.prompt.clone(),
            width,
            height,
            seed: req.seed,
            safety_tolerance: 2,
            output_format: req.output_format.clone(),
            input_image: input_base64,
        };
        if body.output_format.is_empty() {
            body.output_format = "jpeg".into();
        }
        let bytes = self.submit_and_poll(&body).await?;
        Ok(FlashImageResponse {
            image_bytes: bytes,
            mime_type: mime_for(&body.output_format),
            seed_used: req.seed,
            provider_name: "flux".into(),
        })
    }
}

/// BFL Flux 2 Max requires width/height to be multiples of 32 in [256, 2048].
/// Arbitrary dims (like 1080×1920) get silently rounded by BFL, producing an
/// image whose aspect ratio drifts from what the caller requested. For common
/// display ratios, snap to exact-aspect Flux-safe pairs. Otherwise round each
/// axis to the nearest multiple of 32.
fn flux_safe_dims(w: u32, h: u32) -> (u32, u32) {
    if w == 0 || h == 0 {
        return (w, h);
    }
    let ratio = w as f64 / h as f64;
    let approx = |a: f64, b: f64| (a - b).abs() < 0.01;
    if approx(ratio, 9.0 / 16.0) {
        (1152, 2048) // exact 9:16 portrait
    } else if approx(ratio, 16.0 / 9.0) {
        (2048, 1152) // exact 16:9 landscape
    } else if approx(ratio, 3.0 / 4.0) {
        (1536, 2048) // exact 3:4 portrait
    } else if approx(ratio, 4.0 / 3.0) {
        (2048, 1536) // exact 4:3 landscape
    } else if approx(ratio, 1.0) {
        (1024, 1024) // square
    } else {
        let round = |n: u32| ((n + 16) / 32) * 32;
        (round(w).clamp(256, 2048), round(h).clamp(256, 2048))
    }
}

impl FluxProvider {
    /// BFL's `input_image` field expects raw base64 (no `data:...;base64,` prefix,
    /// no URL). Accept whatever the caller passes (signed URL, data URI, or raw
    /// base64) and normalize.
    async fn normalize_input_image(&self, input: &str) -> Result<String> {
        use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
        if input.starts_with("http://") || input.starts_with("https://") {
            let dl = Client::builder().timeout(DOWNLOAD_TIMEOUT).build()?;
            let resp = dl.get(input).send().await?;
            if !resp.status().is_success() {
                return Err(anyhow!(
                    "flux: download input_image HTTP {}",
                    resp.status()
                ));
            }
            let bytes = resp.bytes().await?;
            return Ok(B64.encode(&bytes));
        }
        if let Some(rest) = input.strip_prefix("data:") {
            if let Some((_, b64)) = rest.split_once(',') {
                return Ok(b64.to_string());
            }
        }
        Ok(input.to_string())
    }

    async fn submit_and_poll(&self, body: &SubmitRequest) -> Result<Vec<u8>> {
        let url = format!("{}{}", BASE_URL, MAX_ENDPOINT);
        // Scrub base64 input_image from logs (can be multi-MB) and cap body length.
        let log_body = serde_json::to_value(body).ok().map(|mut v| {
            if let Some(obj) = v.as_object_mut() {
                if let Some(img) = obj.get("input_image").and_then(|v| v.as_str()) {
                    if !img.is_empty() {
                        obj.insert("input_image".into(), serde_json::json!(format!("<base64 {} bytes>", img.len())));
                    }
                }
            }
            let s = v.to_string();
            if s.len() > 2048 {
                format!("{}…[+{} bytes elided]", &s[..2048], s.len() - 2048)
            } else {
                s
            }
        }).unwrap_or_default();
        tracing::info!(url = %url, request_body = %log_body, "flux: POST submit request");
        let started = Instant::now();
        let resp = self
            .client
            .post(&url)
            .header("x-key", &self.api_key)
            .header("Accept", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| {
                tracing::error!(url = %url, error = ?e, elapsed_ms = started.elapsed().as_millis() as u64, "flux: POST transport error");
                anyhow!("flux POST transport: {}", e)
            })?;
        let status = resp.status();
        let raw = resp.text().await.unwrap_or_default();
        let elapsed_ms = started.elapsed().as_millis() as u64;
        tracing::info!(
            status_code = status.as_u16(),
            elapsed_ms,
            response_body = %truncate_log(&raw, 2048),
            "flux: POST submit response"
        );
        if !status.is_success() {
            return Err(anyhow::Error::from(HttpError {
                status_code: status.as_u16(),
                message: raw,
            }));
        }
        let submitted: SubmitResponse = serde_json::from_str(&raw)
            .map_err(|e| anyhow!("decode submit response: {} (raw={})", e, raw))?;
        if submitted.polling_url.is_empty() {
            return Err(anyhow!("flux: no polling_url in response (raw={})", raw));
        }
        tracing::info!(polling_url = %submitted.polling_url, "flux: submitted, polling");
        self.poll_for_result(&submitted.polling_url).await
    }

    async fn poll_for_result(&self, polling_url: &str) -> Result<Vec<u8>> {
        let deadline = Instant::now() + POLL_TIMEOUT;
        let started = Instant::now();
        let mut poll_count = 0u32;
        loop {
            tokio::time::sleep(POLL_INTERVAL).await;
            poll_count += 1;
            if Instant::now() > deadline {
                tracing::error!(
                    polling_url,
                    poll_count,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    max_poll_secs = POLL_TIMEOUT.as_secs(),
                    "flux: polling timed out"
                );
                return Err(anyhow!("flux: polling timed out after {:?}", POLL_TIMEOUT));
            }
            let resp = self
                .client
                .get(polling_url)
                .header("x-key", &self.api_key)
                .header("Accept", "application/json")
                .send()
                .await
                .map_err(|e| {
                    tracing::warn!(polling_url, poll_count, error = ?e, "flux: poll transport error");
                    anyhow!("flux poll transport: {}", e)
                })?;
            let resp_status = resp.status();
            let raw = resp.text().await.unwrap_or_default();
            let parsed: PollResponse = match serde_json::from_str(&raw) {
                Ok(v) => v,
                Err(e) => {
                    tracing::debug!(
                        polling_url,
                        poll_count,
                        status_code = resp_status.as_u16(),
                        raw_body = %truncate_log(&raw, 2048),
                        error = ?e,
                        "flux: poll response unparseable, continuing"
                    );
                    continue;
                }
            };
            tracing::debug!(
                polling_url,
                poll_count,
                status_code = resp_status.as_u16(),
                task_status = %parsed.status,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "flux: poll response"
            );
            match parsed.status.as_str() {
                "Ready" => {
                    if parsed.result.sample.is_empty() {
                        return Err(anyhow!("flux: ready but no sample URL (raw={})", raw));
                    }
                    tracing::info!(
                        polling_url,
                        sample_url = %parsed.result.sample,
                        total_elapsed_ms = started.elapsed().as_millis() as u64,
                        poll_count,
                        "flux: ready, downloading"
                    );
                    return self.download_image(&parsed.result.sample).await;
                }
                "Error" | "Failed" => {
                    tracing::error!(
                        polling_url,
                        poll_count,
                        task_status = %parsed.status,
                        raw_body = %truncate_log(&raw, 2048),
                        "flux: generation terminal failure"
                    );
                    return Err(anyhow!("flux: generation {} (raw={})", parsed.status, truncate_log(&raw, 512)));
                }
                _ => continue,
            }
        }
    }

    async fn download_image(&self, url: &str) -> Result<Vec<u8>> {
        let started = Instant::now();
        tracing::info!(url, "flux: downloading sample");
        let dl = Client::builder().timeout(DOWNLOAD_TIMEOUT).build()?;
        let r = dl.get(url).send().await.map_err(|e| {
            tracing::error!(url, error = ?e, "flux: download transport error");
            anyhow!("flux download transport: {}", e)
        })?;
        let status = r.status();
        if !status.is_success() {
            tracing::error!(url, status_code = status.as_u16(), "flux: download non-2xx");
            return Err(anyhow!("flux download HTTP {}", status));
        }
        let bytes = r.bytes().await?.to_vec();
        tracing::info!(
            url,
            bytes = bytes.len(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "flux: download complete"
        );
        Ok(bytes)
    }
}

fn mime_for(format: &str) -> String {
    match format {
        "png" => "image/png".into(),
        "webp" => "image/webp".into(),
        _ => "image/jpeg".into(),
    }
}

/// Truncate a string to at most `max_bytes` on a char boundary, for log fields.
fn truncate_log(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…[+{} bytes elided]", &s[..end], s.len() - end)
}
