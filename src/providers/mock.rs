use crate::providers::*;
use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use std::sync::OnceLock;

pub struct MockTextProvider;

impl MockTextProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl TextProvider for MockTextProvider {
    async fn generate_text(&self, req: &TextRequest) -> Result<TextResponse> {
        let content = if req.json_mode {
            mock_json_response()
        } else {
            "This is a mock text response from the Scout mock provider.".to_string()
        };
        Ok(TextResponse {
            content,
            tokens_used: TokenUsage {
                prompt_tokens: 100,
                completion_tokens: 200,
                total_tokens: 300,
            },
            model_id: "mock-model".into(),
            provider_name: "mock".into(),
        })
    }
}

pub struct MockImageProvider;

impl MockImageProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ImageProvider for MockImageProvider {
    async fn generate_image(&self, _req: &ImageRequest) -> Result<ImageResponse> {
        Ok(ImageResponse {
            image_data: valid_placeholder_png().to_vec(),
            mime_type: "image/png".into(),
            provider_name: "mock".into(),
        })
    }
}

pub struct MockVideoProvider {
    placeholder_path: String,
}

impl MockVideoProvider {
    pub fn new<S: Into<String>>(path: S) -> Self {
        Self {
            placeholder_path: path.into(),
        }
    }
}

#[async_trait]
impl VideoProvider for MockVideoProvider {
    async fn generate_video(&self, req: &VideoRequest) -> Result<VideoResponse> {
        // Fall back to a tiny placeholder if the placeholder file does not exist.
        let data = tokio::fs::read(&self.placeholder_path)
            .await
            .unwrap_or_else(|_| placeholder_mp4().to_vec());
        let dur = if req.duration_secs == 0 {
            5
        } else {
            req.duration_secs
        };
        Ok(VideoResponse {
            video_data: data,
            mime_type: "video/mp4".into(),
            duration_secs: dur,
            provider_name: "mock".into(),
        })
    }
}

/// Returns a valid 256×256 gray PNG. Large enough to satisfy downstream
/// validators (e.g. Runway rejects reference images smaller than 2×2), while
/// staying cheap to encode. Cached on first call.
fn valid_placeholder_png() -> &'static [u8] {
    static CACHE: OnceLock<Vec<u8>> = OnceLock::new();
    CACHE.get_or_init(|| {
        use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
        let width: u32 = 256;
        let height: u32 = 256;
        let pixels = vec![128u8; (width * height * 3) as usize];
        let mut out = Vec::new();
        PngEncoder::new(&mut out)
            .write_image(&pixels, width, height, ExtendedColorType::Rgb8)
            .expect("mock png encode");
        out
    })
}

fn placeholder_mp4() -> &'static [u8] {
    &[
        0x00, 0x00, 0x00, 0x20, 0x66, 0x74, 0x79, 0x70, 0x69, 0x73, 0x6f, 0x6d, 0x00, 0x00, 0x02,
        0x00, 0x69, 0x73, 0x6f, 0x6d, 0x69, 0x73, 0x6f, 0x32, 0x61, 0x76, 0x63, 0x31, 0x6d, 0x70,
        0x34, 0x31,
    ]
}

/// Canned JSON for the mock text provider. Fields here correspond 1:1 with
/// the JSON shapes the **live** pipeline's prompts ask for — no legacy
/// financial projections, per-decision probability outcomes, or narrative
/// arcs are returned. A single mock payload is valid for every task type;
/// extraneous keys are ignored by each parser.
fn mock_json_response() -> String {
    let v = json!({
        // dashboard (home snapshot — user-level, not per-decision)
        "life_quality_trajectory": {"wellbeing_curve": [{"month": 0, "value": 68}]},
        "life_momentum_score": {"score": 72, "justification": "Mock"},
        "probability_outlook": {"best": 0.25, "likely": 0.55, "worst": 0.20},
        "narrative_summary": "Mock summary",

        // scenario_planner (two-path future) — the authoritative per-decision output
        "path_a": {
            "label": "Take the leap",
            "outcome": "path_a",
            "phases": [
                {"index": 0, "title": "Month 1", "time_label": "Month 1", "scene_prompt": "Mock scene", "motion_prompt": "Mock motion", "edit_prompt": "Mock edit", "narration_text": "Mock narration", "overlay_data": {"primary_metric": {"label": "Savings", "value": "$32k", "trend": "down"}, "secondary_metric": {"label": "Mood", "value": "hopeful", "trend": "up"}, "mood_tag": "hopeful", "risk_level": "medium"}},
                {"index": 1, "title": "Month 3", "time_label": "Month 3", "scene_prompt": "Mock scene", "motion_prompt": "Mock motion", "edit_prompt": "Mock edit", "narration_text": "Mock narration", "overlay_data": {"primary_metric": {"label": "Savings", "value": "$28k", "trend": "down"}, "secondary_metric": {"label": "Mood", "value": "steady", "trend": "flat"}, "mood_tag": "steady", "risk_level": "medium"}},
                {"index": 2, "title": "Month 6", "time_label": "Month 6", "scene_prompt": "Mock scene", "motion_prompt": "Mock motion", "edit_prompt": "Mock edit", "narration_text": "Mock narration", "overlay_data": {"primary_metric": {"label": "Savings", "value": "$30k", "trend": "up"}, "secondary_metric": {"label": "Mood", "value": "confident", "trend": "up"}, "mood_tag": "confident", "risk_level": "low"}}
            ]
        },
        "path_b": {
            "label": "Stay the course",
            "outcome": "path_b",
            "phases": [
                {"index": 0, "title": "Month 1", "time_label": "Month 1", "scene_prompt": "Mock scene", "motion_prompt": "Mock motion", "edit_prompt": "Mock edit", "narration_text": "Mock narration", "overlay_data": {"primary_metric": {"label": "Savings", "value": "$50k", "trend": "up"}, "secondary_metric": {"label": "Mood", "value": "stable", "trend": "flat"}, "mood_tag": "stable", "risk_level": "low"}},
                {"index": 1, "title": "Month 3", "time_label": "Month 3", "scene_prompt": "Mock scene", "motion_prompt": "Mock motion", "edit_prompt": "Mock edit", "narration_text": "Mock narration", "overlay_data": {"primary_metric": {"label": "Savings", "value": "$54k", "trend": "up"}, "secondary_metric": {"label": "Mood", "value": "stable", "trend": "flat"}, "mood_tag": "stable", "risk_level": "low"}},
                {"index": 2, "title": "Month 6", "time_label": "Month 6", "scene_prompt": "Mock scene", "motion_prompt": "Mock motion", "edit_prompt": "Mock edit", "narration_text": "Mock narration", "overlay_data": {"primary_metric": {"label": "Savings", "value": "$60k", "trend": "up"}, "secondary_metric": {"label": "Mood", "value": "steady", "trend": "flat"}, "mood_tag": "steady", "risk_level": "low"}}
            ]
        },
        "shared_context": {"decision_theme": "mock", "time_horizon_months": 6},

        // assumption_extraction
        "assumptions": [
            {"description": "Mock assumption: stable income during transition", "confidence": 0.6, "source": "life_state", "kind": "premise", "grounding": "derived", "category": "financial", "editable": true, "evidence_refs": []}
        ],
        "risks": [
            {"description": "Mock risk: market timing", "likelihood": "medium", "impact": "high", "category": "financial", "linked_assumption_ids": [], "mitigation_hint": "Keep a 3-month runway"}
        ],

        // cinematic_prompt / pipeline_prompt (per-clip overrides)
        "scene_prompt": "Mock scene",
        "motion_prompt": "Mock motion",
        "edit_prompt": "Mock edit",

        // structured_answers_synthesis
        "ai_summary": "Mock synthesized summary.",
        "extracted_context": {},

        // suggested_first_decision
        "decision_text": "Should I take a sabbatical?",
        "time_horizon_months": 12,
        "why_it_matters": "Mock",

        // suggested_first_what_if
        "question": "What if I moved to Lisbon for a year?",
        "mood": "optimistic"
    });
    v.to_string()
}
