//! Provider interfaces and implementations for text, image, and video generation.

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub mod claude;
pub mod flux;
pub mod mock;
pub mod ollama;
pub mod runway;

pub const CLAUDE_OPUS_46: &str = "claude-opus-4-6";
pub const CLAUDE_SONNET_46: &str = "claude-sonnet-4-6";
pub const CLAUDE_HAIKU_45: &str = "claude-haiku-4-5-20251001";

#[derive(Debug, Clone, thiserror::Error)]
#[error("HTTP {status_code}: {message}")]
pub struct HttpError {
    pub status_code: u16,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Default)]
pub struct TextRequest {
    pub system_prompt: String,
    pub user_prompt: String,
    pub cacheable_prefix: String,
    pub model_id: String,
    pub max_tokens: u32,
    pub thinking_budget_tokens: u32,
    pub temperature: f32,
    pub json_mode: bool,
}

#[derive(Debug, Clone, Default)]
pub struct TextResponse {
    pub content: String,
    pub tokens_used: TokenUsage,
    pub model_id: String,
    pub provider_name: String,
}

#[async_trait]
pub trait TextProvider: Send + Sync {
    async fn generate_text(&self, req: &TextRequest) -> Result<TextResponse>;
}

#[derive(Debug, Clone, Default)]
pub struct ImageRequest {
    pub prompt: String,
    pub model: String,
    pub reference_images: Vec<String>,
    pub aspect_ratio: String,
    pub seed: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct ImageResponse {
    pub image_data: Vec<u8>,
    pub mime_type: String,
    pub provider_name: String,
}

#[async_trait]
pub trait ImageProvider: Send + Sync {
    async fn generate_image(&self, req: &ImageRequest) -> Result<ImageResponse>;
}

#[derive(Debug, Clone, Default)]
pub struct FlashImageRequest {
    pub prompt: String,
    pub width: u32,
    pub height: u32,
    pub seed: Option<i64>,
    pub output_format: String,
}

#[derive(Debug, Clone, Default)]
pub struct FlashImageWithInputRequest {
    pub prompt: String,
    pub input_image: String,
    pub width: u32,
    pub height: u32,
    pub seed: Option<i64>,
    pub output_format: String,
}

#[derive(Debug, Clone, Default)]
pub struct FlashImageResponse {
    pub image_bytes: Vec<u8>,
    pub mime_type: String,
    pub seed_used: Option<i64>,
    pub provider_name: String,
}

#[async_trait]
pub trait FlashImageProvider: Send + Sync {
    async fn generate_image(&self, req: &FlashImageRequest) -> Result<FlashImageResponse>;
    async fn generate_image_with_input(
        &self,
        req: &FlashImageWithInputRequest,
    ) -> Result<FlashImageResponse>;
}

#[derive(Debug, Clone, Default)]
pub struct VideoRequest {
    /// Primary motion intent. Runway gen4.x uses this as the full promptText;
    /// Veo-family providers may compose it with `scene_prompt` / `edit_prompt`
    /// / `narration_text` into their preferred 5-part structure.
    pub prompt: String,
    pub duration_secs: u32,
    pub model: String,
    pub aspect_ratio: String,
    pub input_image_url: String,
    pub input_video_url: String,
    pub reference_images: Vec<String>,
    pub seed: Option<i64>,
    /// Scene/composition description (subject in environment, framing, lens).
    /// Providers that compose richer prompts (Veo family) read this; gen4.x
    /// ignores it and uses `prompt` (motion) only.
    pub scene_prompt: String,
    /// Color grade / lighting / atmosphere. Maps to Veo's "Style & Ambiance"
    /// part. Gen4.x ignores it.
    pub edit_prompt: String,
    /// Narration or dialogue line for this phase. Veo 3.1 supports synced
    /// audio and can render this as spoken dialogue via `"..."` syntax.
    /// Gen4.x ignores it (captions live outside the clip).
    pub narration_text: String,
}

#[derive(Debug, Clone, Default)]
pub struct VideoResponse {
    pub video_data: Vec<u8>,
    pub mime_type: String,
    pub duration_secs: u32,
    pub provider_name: String,
}

#[async_trait]
pub trait VideoProvider: Send + Sync {
    async fn generate_video(&self, req: &VideoRequest) -> Result<VideoResponse>;
}

// ---------------------------------------------------------------------------
// Layer D — CinematicShot: structured provider-agnostic phase description
//
// Today's scenario_planner emits `{scene_prompt, motion_prompt, edit_prompt,
// narration_text, overlay_data}` per phase (see `src/prompts.rs`). Those flat
// strings are Runway-friendly but Veo 3.1 prefers a 5-part structure:
// cinematography + subject + action + context + style/ambiance, plus
// narration audio and reference images.
//
// `CinematicShot` is the neutral intermediate representation. It carries
// today's flat strings so the Runway adapter is a pure projection (no
// behavior change), AND the 5-part Veo fields so a future direct Vertex AI
// VeoProvider can consume it without plumbing new types.
//
// This type is groundwork — it is not wired into the live Stage 2 pipeline
// today (that still builds `VideoRequest` directly in `media.rs::execute_stage2`).
// First-frame / last-frame transition slots were removed because Runway-hosted
// veo3 (the live Veo path) takes a single `promptImage` like gen4 i2v; if a
// direct Vertex AI integration is added later, those slots can be re-added.
// ---------------------------------------------------------------------------

/// Provider-neutral description of one cinematic phase. Decouples the
/// scenario_planner's output from any single video provider's prompt shape.
#[derive(Debug, Clone, Default)]
pub struct CinematicShot {
    // --- Legacy flat strings (today's scenario_planner output) ---
    /// First-frame composition: subject in environment, framing, lens. 1–3
    /// sentences. Always populated — this is Runway's primary prompt input.
    pub scene_prompt: String,
    /// What moves during the clip: subject action + camera motion. Always
    /// populated — Runway's Stage 2 motion driver.
    pub motion_prompt: String,
    /// Color grade / lighting / atmosphere adjustments. Optional.
    pub edit_prompt: String,
    /// Short narration line for this phase. Surfaces to Veo's audio track;
    /// Runway currently ignores it (captions live outside the clip).
    pub narration_text: String,

    // --- Veo 3.1 five-part structured prompt ---
    // All optional. When absent, `to_veo_prompt` falls back to concatenating
    // scene_prompt + motion_prompt, which Veo accepts as a single string.
    /// Camera and lens language (e.g. "medium shot, 35mm, shallow depth").
    pub cinematography: Option<String>,
    /// The person or focal subject (e.g. "the subject from the reference").
    /// Veo recommends generic subject language even when a reference image
    /// carries identity (same discipline as Runway).
    pub subject: Option<String>,
    /// What the subject is doing.
    pub action: Option<String>,
    /// Environment and setting.
    pub context: Option<String>,
    /// Aesthetic, mood, lighting grade.
    pub style_ambiance: Option<String>,

    // --- Reference image slot ---
    /// Additional reference images ("ingredients-to-video" in Veo 3.1
    /// parlance). Carried through as-is; providers that don't support
    /// ingredients ignore the slot.
    pub reference_images: Vec<String>,

    // --- Playback/timing metadata ---
    /// Desired clip duration in seconds. Providers clamp to their supported
    /// durations (Runway: 5/10; Veo: 4/6/8; Seedance: 4–15).
    pub duration_secs: u32,
}

impl CinematicShot {
    /// Parse a phase JSON object emitted by `prompts::build_scenario_planner`.
    /// Tolerant of missing optional fields; does not fail on extra fields.
    /// The 5-part Veo fields are read from a nested `"cinematic_shot"` object
    /// when present — today's planner does not emit that object, so those
    /// fields are `None` until the planner's prompt is upgraded (future work).
    pub fn from_phase_json(phase: &serde_json::Value) -> Self {
        let s = |k: &str| {
            phase
                .get(k)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        let nested_s = |root: &str, k: &str| {
            phase
                .get(root)
                .and_then(|v| v.get(k))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|v| v.to_string())
        };
        let refs = phase
            .get("cinematic_shot")
            .and_then(|v| v.get("reference_images"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        Self {
            scene_prompt: s("scene_prompt"),
            motion_prompt: s("motion_prompt"),
            edit_prompt: s("edit_prompt"),
            narration_text: s("narration_text"),
            cinematography: nested_s("cinematic_shot", "cinematography"),
            subject: nested_s("cinematic_shot", "subject"),
            action: nested_s("cinematic_shot", "action"),
            context: nested_s("cinematic_shot", "context"),
            style_ambiance: nested_s("cinematic_shot", "style_ambiance"),
            reference_images: refs,
            duration_secs: 0,
        }
    }

    /// Flatten to a Runway-compatible `VideoRequest`. Motion prompt goes in
    /// `prompt`; `scene_prompt` / `edit_prompt` / `narration_text` ride
    /// alongside in case the provider's branch for the chosen model wants to
    /// compose with them (gen4 ignores; veo3 polish reads them).
    /// Caller is responsible for setting `input_image_url` to whatever
    /// reference image (Stage 1 Flux output, character plate, etc.) Runway's
    /// image-to-video should anchor on.
    pub fn to_runway_video_request(
        &self,
        model: &str,
        aspect_ratio: &str,
        seed: Option<i64>,
    ) -> VideoRequest {
        VideoRequest {
            prompt: self.motion_prompt.clone(),
            duration_secs: self.duration_secs,
            model: model.to_string(),
            aspect_ratio: aspect_ratio.to_string(),
            input_image_url: String::new(),
            input_video_url: String::new(),
            reference_images: self.reference_images.clone(),
            seed,
            scene_prompt: self.scene_prompt.clone(),
            edit_prompt: self.edit_prompt.clone(),
            narration_text: self.narration_text.clone(),
        }
    }

    /// Flatten to a Veo-compatible `VideoRequest`. Assembles the 5-part
    /// structured prompt when those fields are set; otherwise concatenates
    /// `scene_prompt + motion_prompt` as a single string (Veo accepts both
    /// shapes).
    ///
    /// Today, Veo runs through Runway's API (the `veo3` family models in
    /// `RunwayProvider`); Runway's image-to-video endpoint takes a single
    /// `promptImage` like gen4, so the live veo3 path uses
    /// `req.input_image_url` for the anchor frame. If a direct Vertex AI
    /// VeoProvider is added later, it can read the same `VideoRequest`
    /// fields — first-frame/last-frame transitions can be re-added then.
    pub fn to_veo_video_request(
        &self,
        model: &str,
        aspect_ratio: &str,
        seed: Option<i64>,
    ) -> VideoRequest {
        let prompt = if self.cinematography.is_some()
            || self.subject.is_some()
            || self.action.is_some()
            || self.context.is_some()
            || self.style_ambiance.is_some()
        {
            // 5-part assembly. Any missing part is simply omitted.
            [
                self.cinematography.as_deref(),
                self.subject.as_deref(),
                self.action.as_deref(),
                self.context.as_deref(),
                self.style_ambiance.as_deref(),
            ]
            .into_iter()
            .filter_map(|p| p.filter(|s| !s.is_empty()))
            .collect::<Vec<_>>()
            .join(". ")
        } else {
            // Fallback — concatenate flat strings.
            let mut out = self.scene_prompt.clone();
            if !self.motion_prompt.is_empty() {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(&self.motion_prompt);
            }
            out
        };
        VideoRequest {
            prompt,
            duration_secs: self.duration_secs,
            model: model.to_string(),
            aspect_ratio: aspect_ratio.to_string(),
            input_image_url: String::new(),
            input_video_url: String::new(),
            reference_images: self.reference_images.clone(),
            seed,
            scene_prompt: self.scene_prompt.clone(),
            edit_prompt: self.edit_prompt.clone(),
            narration_text: self.narration_text.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cinematic_shot_from_phase_json_reads_flat_fields() {
        let phase = json!({
            "scene_prompt": "a quiet kitchen at dawn",
            "motion_prompt": "slow push-in, subject turns to face the window",
            "edit_prompt": "cool morning grade, soft shadows",
            "narration_text": "The day hasn't decided yet.",
        });
        let shot = CinematicShot::from_phase_json(&phase);
        assert_eq!(shot.scene_prompt, "a quiet kitchen at dawn");
        assert_eq!(
            shot.motion_prompt,
            "slow push-in, subject turns to face the window"
        );
        assert_eq!(shot.edit_prompt, "cool morning grade, soft shadows");
        assert_eq!(shot.narration_text, "The day hasn't decided yet.");
        // No cinematic_shot object → 5-part fields are None.
        assert!(shot.cinematography.is_none());
        assert!(shot.subject.is_none());
    }

    #[test]
    fn cinematic_shot_from_phase_json_reads_nested_veo_fields() {
        let phase = json!({
            "scene_prompt": "a kitchen",
            "motion_prompt": "push in",
            "cinematic_shot": {
                "cinematography": "medium shot, 35mm, shallow DOF",
                "subject": "the subject from the reference",
                "action": "turns toward the window",
                "context": "sunlit kitchen, morning",
                "style_ambiance": "cinematic, warm Portra grade",
                "reference_images": ["https://example.com/a.png", "https://example.com/b.png"],
            },
        });
        let shot = CinematicShot::from_phase_json(&phase);
        assert_eq!(
            shot.cinematography.as_deref(),
            Some("medium shot, 35mm, shallow DOF")
        );
        assert_eq!(
            shot.subject.as_deref(),
            Some("the subject from the reference")
        );
        assert_eq!(shot.reference_images.len(), 2);
    }

    #[test]
    fn cinematic_shot_from_phase_json_tolerates_missing_fields() {
        let phase = json!({});
        let shot = CinematicShot::from_phase_json(&phase);
        assert_eq!(shot.scene_prompt, "");
        assert_eq!(shot.motion_prompt, "");
        assert!(shot.cinematography.is_none());
        assert!(shot.reference_images.is_empty());
    }

    #[test]
    fn to_runway_video_request_carries_motion_and_structured_fields() {
        let shot = CinematicShot {
            scene_prompt: "a quiet kitchen at dawn".into(),
            motion_prompt: "slow push-in".into(),
            edit_prompt: "warm grade".into(),
            duration_secs: 6,
            ..Default::default()
        };
        let req = shot.to_runway_video_request("gen4.5", "1280:720", Some(42));
        // Runway uses motion_prompt as the primary prompt string.
        assert_eq!(req.prompt, "slow push-in");
        // Caller is responsible for setting input_image_url; adapter leaves blank.
        assert_eq!(req.input_image_url, "");
        // Structured fields ride alongside for veo3 polish to consume.
        assert_eq!(req.scene_prompt, "a quiet kitchen at dawn");
        assert_eq!(req.edit_prompt, "warm grade");
        assert_eq!(req.model, "gen4.5");
        assert_eq!(req.aspect_ratio, "1280:720");
        assert_eq!(req.seed, Some(42));
        assert_eq!(req.duration_secs, 6);
    }

    #[test]
    fn to_veo_video_request_uses_structured_prompt_when_parts_present() {
        let shot = CinematicShot {
            cinematography: Some("medium shot, 35mm".into()),
            subject: Some("the subject".into()),
            action: Some("turns slowly".into()),
            context: Some("sunlit kitchen".into()),
            style_ambiance: Some("warm Portra grade".into()),
            duration_secs: 8,
            ..Default::default()
        };
        let req = shot.to_veo_video_request("veo3", "1280:720", None);
        assert!(
            req.prompt.contains("medium shot, 35mm"),
            "expected cinematography in assembled prompt: {}",
            req.prompt
        );
        assert!(req.prompt.contains("turns slowly"));
        assert_eq!(req.duration_secs, 8);
    }

    #[test]
    fn to_veo_video_request_falls_back_to_flat_prompt_when_no_parts() {
        let shot = CinematicShot {
            scene_prompt: "a quiet kitchen".into(),
            motion_prompt: "slow push-in".into(),
            ..Default::default()
        };
        let req = shot.to_veo_video_request("veo3", "1280:720", None);
        assert_eq!(req.prompt, "a quiet kitchen slow push-in");
    }
}

pub type TextProviderRef = Arc<dyn TextProvider>;
pub type ImageProviderRef = Arc<dyn ImageProvider>;
pub type VideoProviderRef = Arc<dyn VideoProvider>;
pub type FlashImageProviderRef = Arc<dyn FlashImageProvider>;

/// Classify whether an error is retryable based on HTTP status or context.
/// 5xx, 429, or network errors => retryable; 4xx => permanent.
pub fn is_retryable_error(err: &anyhow::Error) -> bool {
    if let Some(http) = err.downcast_ref::<HttpError>() {
        let s = http.status_code;
        return s >= 500 || s == 429 || s == 408;
    }
    // network/timeout/connection failures are retryable by default
    true
}
