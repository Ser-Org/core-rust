use crate::providers::*;
use crate::utils::truncate_to_byte_boundary;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

const DEFAULT_BASE_URL: &str = "https://api.dev.runwayml.com/v1";
const POLL_INTERVAL: Duration = Duration::from_secs(5);
const MAX_POLL_DURATION: Duration = Duration::from_secs(60 * 60); // 1h — Runway jobs rarely exceed this.
const HTTP_TIMEOUT: Duration = Duration::from_secs(300); // 5min per request — plenty of headroom for network blips.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600); // 10min — large videos over slow links.
const MAX_CONSEC_ERRORS: u32 = 5;

const MODEL_GEN45: &str = "gen4.5";
const MODEL_GEN4_ALEPH: &str = "gen4_aleph";
const MODEL_VEO3: &str = "veo3";
const MODEL_VEO3_1: &str = "veo3.1";
const MODEL_VEO3_1_FAST: &str = "veo3.1_fast";
const MODEL_SEEDANCE2: &str = "seedance2";
const DEFAULT_I2V_RATIO: &str = "1280:720";
const DEFAULT_VIDEO_DURATION: u32 = 5;
const DEFAULT_TEXT_DURATION: u32 = 4;

const IMAGE_TO_VIDEO_RATIOS: &[&str] = &[
    "1280:720", "720:1280", "1104:832", "960:960", "832:1104", "1584:672",
];
const VIDEO_TO_VIDEO_RATIOS: &[&str] = &[
    "1280:720", "720:1280", "1104:832", "960:960", "832:1104", "1584:672", "848:480", "480:848",
];
const TEXT_TO_VIDEO_RATIOS: &[&str] = &["1280:720", "720:1280", "1080:1920", "1920:1080"];
const TEXT_TO_VIDEO_DURATIONS: &[u32] = &[4, 6, 8];

// Runway-hosted Google Veo 3 family has stricter requirements than gen4.x:
// narrower ratio enum, fixed 8s duration, no seed/moderation/referenceImages.
const VEO3_I2V_RATIOS: &[&str] = &["1280:720", "720:1280", "1080:1920", "1920:1080"];
const VEO3_FIXED_DURATION: u32 = 8;

// Runway-hosted Seedance 2.0: image-to-video only, wider ratio enum, 4-15s
// duration window, no seed/moderation/referenceImages, audio toggle exposed.
const SEEDANCE2_I2V_RATIOS: &[&str] = &[
    "992:432", "864:496", "752:560", "640:640", "560:752", "496:864", "1470:630", "1280:720",
    "1112:834", "960:960", "834:1112", "720:1280",
];
const SEEDANCE2_MIN_DURATION: u32 = 4;
const SEEDANCE2_MAX_DURATION: u32 = 15;
const SEEDANCE2_DEFAULT_DURATION: u32 = 6;

fn is_veo3_family(model: &str) -> bool {
    matches!(model, MODEL_VEO3 | MODEL_VEO3_1 | MODEL_VEO3_1_FAST)
}

fn is_seedance2(model: &str) -> bool {
    model == MODEL_SEEDANCE2
}

/// Runway-specific polish applied to gen4.x image-to-video motion prompts.
/// Keeps the scene base's subject as the only person in frame — the image
/// anchor establishes who appears, and any additional subject invented during
/// animation breaks continuity.
///
/// Lives in the provider (not in the adapter or media pipeline) because the
/// suffix + byte budget are Runway-API concerns; other providers (Veo,
/// Seedance) have different limits and different discipline.
const MOTION_SUFFIX_GEN4: &str = " No other people appear in the frame; do not introduce additional subjects or extras at any point in the clip.";

/// Runway's `promptText` field accepts <=1000 UTF-16 code units; 950 bytes
/// leaves margin for multi-byte chars (em-dashes, smart quotes from Claude).
const PROMPT_TEXT_MAX_BYTES: usize = 950;

/// Append `MOTION_SUFFIX_GEN4` to a motion prompt, trimming the variable
/// portion so the full assembled string fits under `PROMPT_TEXT_MAX_BYTES`.
/// Used for gen4.x image-to-video only. Veo3/Seedance2 families skip this
/// polish (different API limits and different continuity disciplines).
fn polish_gen4_i2v_prompt(raw: &str) -> String {
    let budget = PROMPT_TEXT_MAX_BYTES.saturating_sub(MOTION_SUFFIX_GEN4.len());
    let trimmed = truncate_to_byte_boundary(raw, budget);
    format!("{}{}", trimmed, MOTION_SUFFIX_GEN4)
}

/// Veo-family solo-subject constraint. Same intent as the gen4 suffix
/// (prevent the animation from inventing additional people) but kept as its
/// own constant so we can adjust language independently — Veo responds to
/// different phrasing than gen4.
const SOLO_SUBJECT_CONSTRAINT_VEO: &str =
    " The subject is the only person visible in the frame throughout the clip; do not introduce additional figures.";

/// Compose a Veo-tailored prompt from the scenario_planner's structured
/// fields. Maps loosely to Veo 3.1's recommended 5-part formula
/// (Cinematography + Subject + Action + Context + Style & Ambiance):
///
/// - `scene_prompt` carries cinematography + subject + context (first-frame
///   composition and camera/lens language, per the scenario planner's
///   system prompt).
/// - `motion_prompt` carries action.
/// - `edit_prompt` carries style / ambiance / grade.
/// - `narration_text`, if non-empty, is woven as spoken dialogue using Veo's
///   quoted-line convention.
///
/// Falls back gracefully: missing parts are omitted, empty input still
/// produces the solo-subject constraint. Total output is trimmed to
/// `PROMPT_TEXT_MAX_BYTES`; the trailing constraint is fixed and never
/// trimmed (identity/continuity discipline).
fn polish_veo3_i2v_prompt(
    scene_prompt: &str,
    motion_prompt: &str,
    edit_prompt: &str,
    narration_text: &str,
) -> String {
    let mut segments: Vec<String> = Vec::new();
    if !scene_prompt.is_empty() {
        segments.push(scene_prompt.to_string());
    }
    if !motion_prompt.is_empty() {
        segments.push(motion_prompt.to_string());
    }
    if !edit_prompt.is_empty() {
        segments.push(format!("Style: {}", edit_prompt));
    }
    if !narration_text.is_empty() {
        // Veo's convention for spoken dialogue: a character says, "line."
        segments.push(format!("The subject says, \"{}\"", narration_text));
    }
    let body = segments.join(". ");
    let budget = PROMPT_TEXT_MAX_BYTES.saturating_sub(SOLO_SUBJECT_CONSTRAINT_VEO.len());
    let trimmed = truncate_to_byte_boundary(&body, budget);
    format!("{}{}", trimmed, SOLO_SUBJECT_CONSTRAINT_VEO)
}

pub struct RunwayProvider {
    api_key: String,
    client: Client,
    base_url: String,
    log_interactions: bool,
}

impl RunwayProvider {
    pub fn new(api_key: String, log_interactions: bool) -> Self {
        let client = Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .expect("reqwest client");
        Self {
            api_key,
            client,
            base_url: DEFAULT_BASE_URL.into(),
            log_interactions,
        }
    }
}

#[derive(Serialize, Default)]
struct ContentModeration {
    #[serde(skip_serializing_if = "String::is_empty")]
    public_figure_threshold: String,
}

#[derive(Serialize)]
struct ReferenceImage {
    uri: String,
    tag: String,
}

#[derive(Serialize)]
struct VideoReference {
    #[serde(rename = "type")]
    kind: String,
    uri: String,
}

#[derive(Serialize)]
struct ImageReferenceSimple {
    uri: String,
}

#[derive(Serialize)]
struct TextToImageRequest {
    model: String,
    #[serde(rename = "promptText")]
    prompt_text: String,
    ratio: String,
    #[serde(rename = "referenceImages", skip_serializing_if = "Vec::is_empty")]
    reference_images: Vec<ReferenceImage>,
    #[serde(rename = "contentModeration", skip_serializing_if = "skip_moderation")]
    content_moderation: ContentModeration,
    #[serde(skip_serializing_if = "Option::is_none")]
    seed: Option<i64>,
}

#[derive(Serialize)]
struct ImageToVideoRequest {
    model: String,
    #[serde(rename = "promptImage", skip_serializing_if = "String::is_empty")]
    prompt_image: String,
    #[serde(rename = "promptText", skip_serializing_if = "String::is_empty")]
    prompt_text: String,
    #[serde(rename = "referenceImages", skip_serializing_if = "Vec::is_empty")]
    reference_images: Vec<ImageReferenceSimple>,
    ratio: String,
    duration: u32,
    #[serde(rename = "contentModeration", skip_serializing_if = "skip_moderation")]
    content_moderation: ContentModeration,
    #[serde(skip_serializing_if = "Option::is_none")]
    seed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    audio: Option<bool>,
}

#[derive(Serialize)]
struct VideoToVideoRequest {
    model: String,
    #[serde(rename = "promptText")]
    prompt_text: String,
    #[serde(rename = "videoUri")]
    video_uri: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    references: Vec<VideoReference>,
    #[serde(skip_serializing_if = "String::is_empty")]
    ratio: String,
    #[serde(rename = "contentModeration", skip_serializing_if = "skip_moderation")]
    content_moderation: ContentModeration,
    #[serde(skip_serializing_if = "Option::is_none")]
    seed: Option<i64>,
}

#[derive(Serialize)]
struct TextToVideoRequest {
    model: String,
    #[serde(rename = "promptText")]
    prompt_text: String,
    ratio: String,
    duration: u32,
    #[serde(rename = "contentModeration", skip_serializing_if = "skip_moderation")]
    content_moderation: ContentModeration,
    #[serde(skip_serializing_if = "Option::is_none")]
    seed: Option<i64>,
}

fn skip_moderation(c: &ContentModeration) -> bool {
    c.public_figure_threshold.is_empty()
}

#[derive(Deserialize)]
struct GenerateTaskResponse {
    id: String,
}

#[derive(Deserialize, Default)]
struct TaskStatusResponse {
    id: String,
    status: String,
    #[serde(default)]
    output: Vec<String>,
    #[serde(default)]
    failure: String,
    #[serde(default, rename = "failureCode")]
    failure_code: String,
}

#[derive(Deserialize, Default)]
struct RunwayErrorResponse {
    #[serde(default)]
    error: String,
    #[serde(default)]
    issues: Vec<RunwayValidationErr>,
}

#[derive(Deserialize, Default)]
struct RunwayValidationErr {
    #[serde(default)]
    code: String,
    #[serde(default)]
    path: Vec<String>,
    #[serde(default)]
    message: String,
}

/// Produce a log-safe serialization of a Runway request body. Heavy URI-bearing
/// fields (data URIs, long signed URLs) are replaced with `<elided N chars>` so
/// logs don't drown in base64. Final result is capped to `max_bytes`.
fn log_safe_body<T: Serialize>(body: &T, max_bytes: usize) -> String {
    const HEAVY_FIELDS: &[&str] = &[
        "uri",         // ReferenceImage.uri
        "promptImage", // ImageToVideoRequest.prompt_image
        "videoUri",    // VideoToVideoRequest.video_uri
        "inputImage",  // general
        "input_image", // general
    ];
    let mut v = serde_json::to_value(body).unwrap_or(serde_json::Value::Null);
    scrub_heavy_fields(&mut v, HEAVY_FIELDS, 256);
    let s = serde_json::to_string(&v).unwrap_or_else(|_| "<unserializable>".into());
    if s.len() <= max_bytes {
        s
    } else {
        format!(
            "{}…[+{} bytes elided]",
            &s[..max_bytes],
            s.len() - max_bytes
        )
    }
}

fn scrub_heavy_fields(v: &mut serde_json::Value, heavy_keys: &[&str], max_str_len: usize) {
    match v {
        serde_json::Value::Object(map) => {
            let keys: Vec<String> = map.keys().cloned().collect();
            for k in keys {
                let is_heavy = heavy_keys.contains(&k.as_str());
                if let Some(entry) = map.get_mut(&k) {
                    if is_heavy {
                        if let Some(s) = entry.as_str() {
                            let prefix = if s.starts_with("data:") {
                                "data uri"
                            } else if s.starts_with("http") {
                                "url"
                            } else {
                                "string"
                            };
                            if s.len() > max_str_len {
                                *entry = serde_json::json!(format!(
                                    "<{} elided {} chars>",
                                    prefix,
                                    s.len()
                                ));
                                continue;
                            }
                        }
                    }
                    scrub_heavy_fields(entry, heavy_keys, max_str_len);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                scrub_heavy_fields(item, heavy_keys, max_str_len);
            }
        }
        _ => {}
    }
}

/// Truncate a string to at most `max_bytes`, backing up to a UTF-8 char
/// boundary. Used on response bodies that may contain long URLs or base64.
fn truncate_body_for_log(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…[+{} bytes elided]", &s[..end], s.len() - end)
}

fn format_runway_error(raw: &str) -> String {
    let parsed: Option<RunwayErrorResponse> = serde_json::from_str(raw).ok();
    let Some(resp) = parsed else {
        return raw.to_string();
    };
    let mut parts: Vec<String> = Vec::new();
    if !resp.error.is_empty() {
        parts.push(resp.error);
    }
    for issue in resp.issues {
        let path = issue.path.join(".");
        let line = match (
            path.is_empty(),
            issue.message.is_empty(),
            issue.code.is_empty(),
        ) {
            (false, false, _) => format!("{}: {}", path, issue.message),
            (false, true, false) => format!("{}: {}", path, issue.code),
            (false, true, true) => path,
            (true, false, _) => issue.message,
            (true, true, false) => issue.code,
            _ => continue,
        };
        parts.push(line);
    }
    if parts.is_empty() {
        raw.to_string()
    } else {
        parts.join("; ")
    }
}

impl RunwayProvider {
    async fn post_task<T: Serialize>(&self, endpoint: &str, body: &T) -> Result<String> {
        let url = format!("{}{}", self.base_url, endpoint);
        let body_json = log_safe_body(body, 2048);
        tracing::info!(
            endpoint,
            url = %url,
            request_body = %body_json,
            "runway: POST task request"
        );
        let started = Instant::now();
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .header("X-Runway-Version", "2024-11-06")
            .json(body)
            .send()
            .await
            .map_err(|e| {
                tracing::error!(endpoint, url = %url, error = ?e, elapsed_ms = started.elapsed().as_millis() as u64, "runway: POST transport error");
                anyhow!("runway POST {} transport: {}", endpoint, e)
            })?;
        let status = resp.status();
        let raw = resp.text().await.unwrap_or_default();
        let elapsed_ms = started.elapsed().as_millis() as u64;
        tracing::info!(
            endpoint,
            status_code = status.as_u16(),
            elapsed_ms,
            response_body = %truncate_body_for_log(&raw, 2048),
            "runway: POST task response"
        );
        if status != StatusCode::OK && status != StatusCode::CREATED {
            let msg = format_runway_error(&raw);
            tracing::warn!(endpoint, status_code = status.as_u16(), error = %msg, raw_body = %raw, "runway: task POST rejected");
            return Err(anyhow::Error::from(HttpError {
                status_code: status.as_u16(),
                message: msg,
            }));
        }
        let r: GenerateTaskResponse = serde_json::from_str(&raw)
            .map_err(|e| anyhow!("decode task response: {} (raw={})", e, raw))?;
        if r.id.is_empty() {
            return Err(anyhow!("runway: response missing task ID (raw={})", raw));
        }
        tracing::info!(endpoint, task_id = %r.id, elapsed_ms, "runway: task accepted");
        Ok(r.id)
    }

    async fn poll_until_done(&self, task_id: &str) -> Result<TaskStatusResponse> {
        tracing::info!(task_id, "runway: polling task");
        let deadline = Instant::now() + MAX_POLL_DURATION;
        let started = Instant::now();
        let mut consec_errors = 0u32;
        let mut throttled_logged = false;
        let mut poll_count = 0u32;
        loop {
            tokio::time::sleep(POLL_INTERVAL).await;
            poll_count += 1;
            if Instant::now() > deadline {
                tracing::error!(
                    task_id,
                    poll_count,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    max_poll_secs = MAX_POLL_DURATION.as_secs(),
                    "runway: polling exceeded deadline"
                );
                return Err(anyhow!("polling exceeded {:?}", MAX_POLL_DURATION));
            }
            match self.get_task_status(task_id).await {
                Ok(s) => {
                    consec_errors = 0;
                    tracing::info!(
                        task_id,
                        poll_count,
                        status = %s.status,
                        elapsed_ms = started.elapsed().as_millis() as u64,
                        "runway: task status"
                    );
                    match s.status.as_str() {
                        "SUCCEEDED" | "FAILED" | "CANCELLED" => {
                            tracing::info!(
                                task_id,
                                poll_count,
                                final_status = %s.status,
                                failure_code = %s.failure_code,
                                failure = %s.failure,
                                output_count = s.output.len(),
                                total_elapsed_ms = started.elapsed().as_millis() as u64,
                                "runway: task terminal state"
                            );
                            return Ok(s);
                        }
                        "PENDING" | "RUNNING" => continue,
                        "THROTTLED" => {
                            if !throttled_logged {
                                tracing::info!(
                                    task_id,
                                    "runway: task throttled, continuing to poll"
                                );
                                throttled_logged = true;
                            }
                            continue;
                        }
                        other => return Err(anyhow!("runway: unknown status {}", other)),
                    }
                }
                Err(e) => {
                    consec_errors += 1;
                    tracing::warn!(
                        task_id,
                        poll_count,
                        consec_errors,
                        error = ?e,
                        "runway: poll error"
                    );
                    if consec_errors >= MAX_CONSEC_ERRORS {
                        return Err(anyhow!(
                            "polling task {}: {} consecutive errors",
                            task_id,
                            consec_errors
                        ));
                    }
                }
            }
        }
    }

    async fn get_task_status(&self, task_id: &str) -> Result<TaskStatusResponse> {
        let url = format!("{}/tasks/{}", self.base_url, task_id);
        let started = Instant::now();
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.api_key)
            .header("X-Runway-Version", "2024-11-06")
            .send()
            .await
            .map_err(|e| {
                tracing::warn!(task_id, error = ?e, elapsed_ms = started.elapsed().as_millis() as u64, "runway: GET status transport error");
                anyhow!("runway GET status transport: {}", e)
            })?;
        let status = resp.status();
        let raw = resp.text().await.unwrap_or_default();
        let elapsed_ms = started.elapsed().as_millis() as u64;
        tracing::debug!(
            task_id,
            status_code = status.as_u16(),
            elapsed_ms,
            response_body = %truncate_body_for_log(&raw, 2048),
            "runway: GET status response"
        );
        if !status.is_success() {
            return Err(anyhow!(
                "runway: get status {} -> {} (raw={})",
                task_id,
                status,
                raw
            ));
        }
        let s: TaskStatusResponse = serde_json::from_str(&raw)
            .map_err(|e| anyhow!("decode status: {} (raw={})", e, raw))?;
        Ok(s)
    }

    async fn download_output(&self, url: &str) -> Result<Vec<u8>> {
        let started = Instant::now();
        tracing::info!(url, "runway: downloading output");
        let dl = Client::builder().timeout(DOWNLOAD_TIMEOUT).build()?;
        let resp = dl.get(url).send().await.map_err(|e| {
            tracing::error!(url, error = ?e, elapsed_ms = started.elapsed().as_millis() as u64, "runway: download transport error");
            anyhow!("runway download transport: {}", e)
        })?;
        let status = resp.status();
        if !status.is_success() {
            tracing::error!(
                url,
                status_code = status.as_u16(),
                "runway: download non-2xx"
            );
            return Err(anyhow!("download returned {}", status));
        }
        let bytes = resp.bytes().await?.to_vec();
        tracing::info!(
            url,
            bytes = bytes.len(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "runway: download complete"
        );
        Ok(bytes)
    }
}

fn validate_allowed(endpoint: &str, field: &str, value: &str, allowed: &[&str]) -> Result<()> {
    if allowed.contains(&value) {
        return Ok(());
    }
    Err(anyhow::Error::from(HttpError {
        status_code: 400,
        message: format!(
            "{}: invalid {} \"{}\" (expected one of {:?})",
            endpoint, field, value, allowed
        ),
    }))
}

fn validate_i2v(ratio: &str, duration: u32) -> Result<()> {
    validate_allowed("image_to_video", "ratio", ratio, IMAGE_TO_VIDEO_RATIOS)?;
    if !(2..=10).contains(&duration) {
        return Err(anyhow::Error::from(HttpError {
            status_code: 400,
            message: format!(
                "image_to_video: invalid duration {} (expected 2-10)",
                duration
            ),
        }));
    }
    Ok(())
}

fn validate_veo3_i2v(ratio: &str, duration: u32) -> Result<()> {
    validate_allowed("image_to_video[veo3]", "ratio", ratio, VEO3_I2V_RATIOS)?;
    if duration != VEO3_FIXED_DURATION {
        return Err(anyhow::Error::from(HttpError {
            status_code: 400,
            message: format!(
                "image_to_video[veo3]: invalid duration {} (must be exactly {})",
                duration, VEO3_FIXED_DURATION
            ),
        }));
    }
    Ok(())
}

fn validate_seedance2_i2v(ratio: &str, duration: u32) -> Result<()> {
    validate_allowed(
        "image_to_video[seedance2]",
        "ratio",
        ratio,
        SEEDANCE2_I2V_RATIOS,
    )?;
    if !(SEEDANCE2_MIN_DURATION..=SEEDANCE2_MAX_DURATION).contains(&duration) {
        return Err(anyhow::Error::from(HttpError {
            status_code: 400,
            message: format!(
                "image_to_video[seedance2]: invalid duration {} (expected {}-{})",
                duration, SEEDANCE2_MIN_DURATION, SEEDANCE2_MAX_DURATION
            ),
        }));
    }
    Ok(())
}

fn validate_t2v(ratio: &str, duration: u32) -> Result<()> {
    validate_allowed("text_to_video", "ratio", ratio, TEXT_TO_VIDEO_RATIOS)?;
    if !TEXT_TO_VIDEO_DURATIONS.contains(&duration) {
        return Err(anyhow::Error::from(HttpError {
            status_code: 400,
            message: format!(
                "text_to_video: invalid duration {} (expected one of {:?})",
                duration, TEXT_TO_VIDEO_DURATIONS
            ),
        }));
    }
    Ok(())
}

#[async_trait]
impl ImageProvider for RunwayProvider {
    async fn generate_image(&self, req: &ImageRequest) -> Result<ImageResponse> {
        let model = if req.model.is_empty() {
            "gen4_image"
        } else {
            &req.model
        };
        let ratio = if req.aspect_ratio.is_empty() {
            "1920:1080"
        } else {
            &req.aspect_ratio
        };
        let refs: Vec<ReferenceImage> = req
            .reference_images
            .iter()
            .enumerate()
            .map(|(i, u)| ReferenceImage {
                uri: u.clone(),
                tag: format!("ref{}", i),
            })
            .collect();
        let body = TextToImageRequest {
            model: model.to_string(),
            prompt_text: req.prompt.clone(),
            ratio: ratio.to_string(),
            reference_images: refs,
            content_moderation: ContentModeration {
                public_figure_threshold: "auto".into(),
            },
            seed: req.seed,
        };
        let task_id = self.post_task("/text_to_image", &body).await?;
        let status = self.poll_until_done(&task_id).await?;
        if status.status == "FAILED" || status.status == "CANCELLED" {
            return Err(anyhow!(
                "runway image {} [task_id={} code={}]: {}",
                status.status,
                task_id,
                if status.failure_code.is_empty() {
                    "<none>"
                } else {
                    &status.failure_code
                },
                status.failure
            ));
        }
        if status.output.is_empty() {
            return Err(anyhow!("runway image succeeded but no output URLs"));
        }
        let data = self.download_output(&status.output[0]).await?;
        Ok(ImageResponse {
            image_data: data,
            mime_type: "image/png".into(),
            provider_name: "runway".into(),
        })
    }
}

#[async_trait]
impl FlashImageProvider for RunwayProvider {
    async fn generate_image(&self, req: &FlashImageRequest) -> Result<FlashImageResponse> {
        let body = TextToImageRequest {
            model: "gen4_image".into(),
            prompt_text: req.prompt.clone(),
            ratio: format!("{}:{}", req.width, req.height),
            reference_images: vec![],
            content_moderation: ContentModeration {
                public_figure_threshold: "auto".into(),
            },
            seed: req.seed,
        };
        let task_id = self.post_task("/text_to_image", &body).await?;
        let status = self.poll_until_done(&task_id).await?;
        if status.status == "FAILED" || status.status == "CANCELLED" {
            return Err(anyhow!(
                "runway gen4_image {} [task_id={} code={}]: {}",
                status.status,
                task_id,
                if status.failure_code.is_empty() {
                    "<none>"
                } else {
                    &status.failure_code
                },
                status.failure
            ));
        }
        if status.output.is_empty() {
            return Err(anyhow!("runway gen4_image succeeded but no output URLs"));
        }
        let data = self.download_output(&status.output[0]).await?;
        tracing::info!(provider = "runway-gen4_image", task_id = %task_id, bytes = data.len(), "flash: image ready");
        Ok(FlashImageResponse {
            image_bytes: data,
            mime_type: "image/png".into(),
            seed_used: req.seed,
            provider_name: "runway-gen4_image".into(),
        })
    }

    async fn generate_image_with_input(
        &self,
        req: &FlashImageWithInputRequest,
    ) -> Result<FlashImageResponse> {
        let body = TextToImageRequest {
            model: "gen4_image".into(),
            prompt_text: req.prompt.clone(),
            ratio: format!("{}:{}", req.width, req.height),
            reference_images: vec![ReferenceImage {
                uri: req.input_image.clone(),
                tag: "subject".into(),
            }],
            content_moderation: ContentModeration {
                public_figure_threshold: "auto".into(),
            },
            seed: req.seed,
        };
        let task_id = self.post_task("/text_to_image", &body).await?;
        let status = self.poll_until_done(&task_id).await?;
        if status.status == "FAILED" || status.status == "CANCELLED" {
            return Err(anyhow!(
                "runway gen4_image {} [task_id={} code={}]: {}",
                status.status,
                task_id,
                if status.failure_code.is_empty() {
                    "<none>"
                } else {
                    &status.failure_code
                },
                status.failure
            ));
        }
        if status.output.is_empty() {
            return Err(anyhow!("runway gen4_image succeeded but no output URLs"));
        }
        let data = self.download_output(&status.output[0]).await?;
        tracing::info!(provider = "runway-gen4_image", task_id = %task_id, bytes = data.len(), "flash: image ready");
        Ok(FlashImageResponse {
            image_bytes: data,
            mime_type: "image/png".into(),
            seed_used: req.seed,
            provider_name: "runway-gen4_image".into(),
        })
    }
}

#[async_trait]
impl VideoProvider for RunwayProvider {
    async fn generate_video(&self, req: &VideoRequest) -> Result<VideoResponse> {
        let moderation = ContentModeration {
            public_figure_threshold: "auto".into(),
        };
        let task_id = if !req.input_video_url.is_empty() {
            let model = if req.model.is_empty() {
                MODEL_GEN4_ALEPH
            } else {
                &req.model
            };
            let ratio = req.aspect_ratio.clone();
            if !ratio.is_empty() {
                validate_allowed("video_to_video", "ratio", &ratio, VIDEO_TO_VIDEO_RATIOS)?;
            }
            let refs: Vec<VideoReference> = req
                .reference_images
                .iter()
                .map(|u| VideoReference {
                    kind: "image".into(),
                    uri: u.clone(),
                })
                .collect();
            let body = VideoToVideoRequest {
                model: model.to_string(),
                prompt_text: req.prompt.clone(),
                video_uri: req.input_video_url.clone(),
                references: refs,
                ratio,
                content_moderation: moderation,
                seed: req.seed,
            };
            self.post_task("/video_to_video", &body).await?
        } else if !req.input_image_url.is_empty() {
            let model = if req.model.is_empty() {
                MODEL_GEN45
            } else {
                &req.model
            };
            let ratio = if req.aspect_ratio.is_empty() {
                DEFAULT_I2V_RATIO.to_string()
            } else {
                req.aspect_ratio.clone()
            };

            let body = if is_veo3_family(model) {
                // veo3 family: fixed 8s duration, no seed/moderation/refs.
                // Caller's duration is coerced (veo3 rejects any other value).
                if req.duration_secs != VEO3_FIXED_DURATION {
                    tracing::debug!(
                        model,
                        requested = req.duration_secs,
                        forced = VEO3_FIXED_DURATION,
                        "runway: coercing duration to veo3's fixed value"
                    );
                }
                if !req.reference_images.is_empty() {
                    tracing::warn!(
                        model,
                        count = req.reference_images.len(),
                        "runway: veo3 does not accept referenceImages — dropping"
                    );
                }
                validate_veo3_i2v(&ratio, VEO3_FIXED_DURATION)?;
                // Veo-tailored prompt: composes scene + motion + edit (style)
                // + narration (as spoken dialogue) into Veo's preferred 5-part
                // shape. Falls back gracefully when fields are empty.
                let polished_prompt = polish_veo3_i2v_prompt(
                    &req.scene_prompt,
                    &req.prompt,
                    &req.edit_prompt,
                    &req.narration_text,
                );
                tracing::debug!(
                    model,
                    scene_bytes = req.scene_prompt.len(),
                    motion_bytes = req.prompt.len(),
                    narration_bytes = req.narration_text.len(),
                    polished_bytes = polished_prompt.len(),
                    "runway: applied veo3 i2v 5-part prompt polish"
                );
                ImageToVideoRequest {
                    model: model.to_string(),
                    prompt_image: req.input_image_url.clone(),
                    prompt_text: polished_prompt,
                    reference_images: vec![],
                    ratio,
                    duration: VEO3_FIXED_DURATION,
                    content_moderation: ContentModeration::default(),
                    seed: None,
                    audio: None,
                }
            } else if is_seedance2(model) {
                // seedance2: image-to-video only, audio explicitly off,
                // duration sourced from caller (validated to 4-15).
                if !req.reference_images.is_empty() {
                    tracing::warn!(
                        model,
                        count = req.reference_images.len(),
                        "runway: seedance2 does not accept referenceImages — dropping"
                    );
                }
                let duration = if req.duration_secs < SEEDANCE2_MIN_DURATION {
                    SEEDANCE2_DEFAULT_DURATION
                } else {
                    req.duration_secs
                };
                validate_seedance2_i2v(&ratio, duration)?;
                ImageToVideoRequest {
                    model: model.to_string(),
                    prompt_image: req.input_image_url.clone(),
                    prompt_text: req.prompt.clone(),
                    reference_images: vec![],
                    ratio,
                    duration,
                    content_moderation: ContentModeration::default(),
                    seed: None,
                    audio: Some(false),
                }
            } else {
                let duration = if req.duration_secs < 2 {
                    DEFAULT_VIDEO_DURATION
                } else {
                    req.duration_secs
                };
                validate_i2v(&ratio, duration)?;
                let refs: Vec<ImageReferenceSimple> = req
                    .reference_images
                    .iter()
                    .map(|u| ImageReferenceSimple { uri: u.clone() })
                    .collect();
                // gen4.x i2v polish: append the no-other-subjects suffix and
                // trim to fit promptText's UTF-16 cap. Applied only for
                // gen4.x models — veo3/seedance2 branches above skip this.
                let polished_prompt = polish_gen4_i2v_prompt(&req.prompt);
                tracing::debug!(
                    model,
                    raw_bytes = req.prompt.len(),
                    polished_bytes = polished_prompt.len(),
                    "runway: applied gen4.x i2v motion-prompt polish"
                );
                ImageToVideoRequest {
                    model: model.to_string(),
                    prompt_image: req.input_image_url.clone(),
                    prompt_text: polished_prompt,
                    reference_images: refs,
                    ratio,
                    duration,
                    content_moderation: moderation,
                    seed: req.seed,
                    audio: None,
                }
            };
            self.post_task("/image_to_video", &body).await?
        } else {
            let model = if req.model.is_empty() {
                MODEL_GEN45
            } else {
                &req.model
            };
            if model == MODEL_GEN45 {
                let ratio = if req.aspect_ratio.is_empty() {
                    DEFAULT_I2V_RATIO.to_string()
                } else {
                    req.aspect_ratio.clone()
                };
                let duration = if req.duration_secs < 2 {
                    DEFAULT_VIDEO_DURATION
                } else {
                    req.duration_secs
                };
                validate_i2v(&ratio, duration)?;
                let body = ImageToVideoRequest {
                    model: model.to_string(),
                    prompt_image: String::new(),
                    prompt_text: req.prompt.clone(),
                    reference_images: vec![],
                    ratio,
                    duration,
                    content_moderation: moderation,
                    seed: req.seed,
                    audio: None,
                };
                self.post_task("/image_to_video", &body).await?
            } else {
                let ratio = if req.aspect_ratio.is_empty() {
                    DEFAULT_I2V_RATIO.to_string()
                } else {
                    req.aspect_ratio.clone()
                };
                let duration = if req.duration_secs == 0 {
                    DEFAULT_TEXT_DURATION
                } else {
                    req.duration_secs
                };
                validate_t2v(&ratio, duration)?;
                let body = TextToVideoRequest {
                    model: model.to_string(),
                    prompt_text: req.prompt.clone(),
                    ratio,
                    duration,
                    content_moderation: moderation,
                    seed: req.seed,
                };
                self.post_task("/text_to_video", &body).await?
            }
        };

        let status = self.poll_until_done(&task_id).await?;
        if status.status == "FAILED" || status.status == "CANCELLED" {
            return Err(anyhow!(
                "runway video {} [task_id={} code={}]: {}",
                status.status,
                task_id,
                if status.failure_code.is_empty() {
                    "<none>"
                } else {
                    &status.failure_code
                },
                status.failure
            ));
        }
        if status.output.is_empty() {
            return Err(anyhow!("runway video succeeded but no output URLs"));
        }
        let data = self.download_output(&status.output[0]).await?;
        Ok(VideoResponse {
            video_data: data,
            mime_type: "video/mp4".into(),
            duration_secs: req.duration_secs,
            provider_name: "runway".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polish_gen4_i2v_prompt_appends_suffix_and_fits_under_cap() {
        let raw = "the subject turns slowly toward the window";
        let out = polish_gen4_i2v_prompt(raw);
        // Always ends with the motion suffix (recency for Runway's attention).
        assert!(
            out.ends_with(MOTION_SUFFIX_GEN4),
            "polish output did not end with motion suffix: {out}"
        );
        // Never exceeds Runway's promptText byte budget.
        assert!(
            out.len() <= PROMPT_TEXT_MAX_BYTES,
            "polish output {} bytes exceeded cap {}",
            out.len(),
            PROMPT_TEXT_MAX_BYTES
        );
        // Raw content survives intact for short inputs.
        assert!(out.starts_with(raw));
    }

    #[test]
    fn polish_gen4_i2v_prompt_trims_variable_portion_under_cap() {
        // Oversized motion prompt must be trimmed so the fixed suffix still fits.
        let raw = "a".repeat(5_000);
        let out = polish_gen4_i2v_prompt(&raw);
        assert!(
            out.len() <= PROMPT_TEXT_MAX_BYTES,
            "polish output {} bytes exceeded cap {}",
            out.len(),
            PROMPT_TEXT_MAX_BYTES
        );
        // Suffix is never trimmed.
        assert!(out.ends_with(MOTION_SUFFIX_GEN4));
    }

    #[test]
    fn polish_gen4_i2v_prompt_preserves_utf8_boundaries() {
        // Multi-byte chars at the trim point must not panic or produce
        // invalid UTF-8 (em-dashes from Claude output are a common case).
        let raw = "turn —".repeat(500);
        let out = polish_gen4_i2v_prompt(&raw);
        assert!(out.is_char_boundary(out.len())); // trivially true for &str
        assert!(out.ends_with(MOTION_SUFFIX_GEN4));
        assert!(out.len() <= PROMPT_TEXT_MAX_BYTES);
    }

    #[test]
    fn polish_gen4_i2v_prompt_preserves_empty_input() {
        let out = polish_gen4_i2v_prompt("");
        // Empty input still produces the suffix — downstream constraint
        // survives even when the planner emitted nothing.
        assert_eq!(out, MOTION_SUFFIX_GEN4);
    }

    #[test]
    fn polish_veo3_i2v_prompt_composes_all_five_parts() {
        let out = polish_veo3_i2v_prompt(
            "Medium shot, 35mm. The subject stands in a sunlit kitchen, shallow depth of field",
            "slow push-in, the subject turns to face the window",
            "warm Kodak Portra grade, morning light",
            "Maybe I should just go",
        );
        assert!(out.contains("Medium shot, 35mm"));
        assert!(out.contains("slow push-in"));
        assert!(out.contains("Style: warm Kodak Portra grade"));
        // Narration rendered as Veo-style spoken dialogue.
        assert!(
            out.contains("The subject says, \"Maybe I should just go\""),
            "narration not woven as dialogue: {out}"
        );
        // Solo-subject constraint is the final clause (recency).
        assert!(out.ends_with(SOLO_SUBJECT_CONSTRAINT_VEO));
    }

    #[test]
    fn polish_veo3_i2v_prompt_skips_empty_segments() {
        // Only scene + motion populated. No "Style:" label, no dialogue line.
        let out = polish_veo3_i2v_prompt("a quiet kitchen at dawn", "slow push-in", "", "");
        assert!(!out.contains("Style:"));
        assert!(!out.contains("The subject says"));
        assert!(out.contains("a quiet kitchen at dawn"));
        assert!(out.contains("slow push-in"));
        assert!(out.ends_with(SOLO_SUBJECT_CONSTRAINT_VEO));
    }

    #[test]
    fn polish_veo3_i2v_prompt_all_empty_still_emits_constraint() {
        let out = polish_veo3_i2v_prompt("", "", "", "");
        // Zero content → just the solo-subject constraint; any provider
        // downstream gets a valid promptText rather than an empty string.
        assert_eq!(out, SOLO_SUBJECT_CONSTRAINT_VEO);
    }

    #[test]
    fn polish_veo3_i2v_prompt_respects_byte_cap_and_preserves_constraint() {
        let scene = "a".repeat(5_000);
        let out = polish_veo3_i2v_prompt(&scene, "", "", "");
        assert!(
            out.len() <= PROMPT_TEXT_MAX_BYTES,
            "veo polish output {} bytes exceeded cap {}",
            out.len(),
            PROMPT_TEXT_MAX_BYTES
        );
        // Constraint tail is never trimmed.
        assert!(out.ends_with(SOLO_SUBJECT_CONSTRAINT_VEO));
    }

    #[test]
    fn polish_veo3_i2v_prompt_preserves_utf8_boundaries() {
        let scene = "turn —".repeat(500);
        let out = polish_veo3_i2v_prompt(&scene, "", "", "");
        assert!(out.is_char_boundary(out.len()));
        assert!(out.ends_with(SOLO_SUBJECT_CONSTRAINT_VEO));
    }
}
