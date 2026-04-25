//! Media pipeline — 2-stage Precision Sequence
//!
//! Stage 1: Flux 2 Max — character-in-scene composition (input: character plate)
//! Stage 2: Runway gen4.5 — image-to-video animation (final output)
//!
//! Stage 1 was previously Runway gen4_image; Flux holds identity better when
//! given a single input image, and Runway takes over from the animation step.
//! A former Stage 3 (gen4_aleph video-to-video) was removed: aleph's creative
//! re-synthesis distorted character identity. gen4.5 is now the terminal stage.

use crate::objectstore::ObjectStore;
use crate::providers::{
    FlashImageProviderRef, FlashImageWithInputRequest, ImageProviderRef, VideoProviderRef,
    VideoRequest,
};
use anyhow::{anyhow, Context, Result};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

pub const DEFAULT_PIPELINE_VIDEO_ASPECT_RATIO: &str = "1280:720";

/// Flux-safe exact 16:9 landscape pair (multiples of 32 in [256, 2048]).
/// See `flux_safe_dims` in src/providers/flux.rs.
const STAGE1_FLUX_WIDTH: u32 = 2048;
const STAGE1_FLUX_HEIGHT: u32 = 1152;

/// Stage 1 (Flux) prompt assembly — including the prefix, suffix, constraints,
/// and identity-lock tail — lives in `prompts::build_flux_scene_prompt`.
/// Identity-preservation reinforcement and user-state-block injection happen
/// there so all prompt construction has one home.
///
/// Stage 2 motion-prompt polish (no-other-subjects suffix + byte-budget
/// trimming) is provider-specific and now lives in the provider:
/// `src/providers/runway.rs::polish_gen4_i2v_prompt`. Media pipeline passes
/// the motion prompt through unchanged; provider applies its own finishing.

#[derive(Clone)]
pub struct MediaPipeline {
    pub object_store: Arc<ObjectStore>,
    pub image_provider: ImageProviderRef,
    pub flash_image_provider: FlashImageProviderRef,
    pub video_provider: VideoProviderRef,
    pub media_bucket: String,
    /// Model id passed to the video provider at stage 2. Populated from
    /// `config::Config::video_model` (derived from `VIDEO_PROVIDER`). Falls
    /// back to `"gen4.5"` when empty.
    pub video_model: String,
}

impl MediaPipeline {
    pub fn new(
        object_store: Arc<ObjectStore>,
        image_provider: ImageProviderRef,
        flash_image_provider: FlashImageProviderRef,
        video_provider: VideoProviderRef,
        media_bucket: String,
        video_model: String,
    ) -> Self {
        Self {
            object_store,
            image_provider,
            flash_image_provider,
            video_provider,
            media_bucket,
            video_model,
        }
    }

    pub async fn execute(&self, input: PipelineInput) -> Result<PipelineOutput> {
        let started = std::time::Instant::now();
        let seed: i64 = input.seed.unwrap_or_else(|| rand::random::<u32>() as i64);
        tracing::info!(
            simulation_id = %input.simulation_id,
            user_id = %input.user_id,
            clip_tag = %input.clip_tag,
            scene_prompt_len = input.scene_prompt.len(),
            motion_prompt_len = input.motion_prompt.len(),
            has_profile_photo = !input.profile_photo_url.is_empty(),
            has_character_plate = !input.character_plate_url.is_empty(),
            duration_secs = input.duration_secs,
            seed,
            "media_pipeline: execute start"
        );
        let scene = self
            .execute_stage1(&input, seed)
            .await
            .context("stage 1 (base)")?;
        let motion = self
            .execute_stage2(&input, &scene, seed)
            .await
            .context("stage 2 (motion)")?;

        tracing::info!(
            simulation_id = %input.simulation_id,
            clip_tag = %input.clip_tag,
            total_elapsed_ms = started.elapsed().as_millis() as u64,
            "media_pipeline: execute complete"
        );
        Ok(PipelineOutput {
            scene_image: scene,
            motion_video: motion.clone(),
            final_video: motion,
        })
    }

    async fn execute_stage1(&self, input: &PipelineInput, seed: i64) -> Result<PipelineArtifact> {
        let started = std::time::Instant::now();
        if input.character_plate_url.is_empty() {
            return Err(anyhow!(
                "missing character plate reference for stage 1 (Flux); the pipeline requires it to anchor identity"
            ));
        }

        // Stage 1 prompt assembly — including prefix, user-state injection,
        // suffix, constraints, and identity-lock tail — is in prompts.rs.
        // The variable middle (user_state_block + scene_prompt) is byte-trimmed
        // there to keep the assembled prompt under prompts::PIPELINE_PROMPT_MAX_BYTES.
        let final_prompt =
            crate::prompts::build_flux_scene_prompt(&input.user_state_block, &input.scene_prompt);

        tracing::info!(
            simulation_id = %input.simulation_id,
            clip_tag = %input.clip_tag,
            width = STAGE1_FLUX_WIDTH,
            height = STAGE1_FLUX_HEIGHT,
            seed,
            scene_prompt_preview = %truncate_for_log(&input.scene_prompt, 200),
            user_state_block_preview = %truncate_for_log(&input.user_state_block, 200),
            user_state_block_bytes = input.user_state_block.len(),
            final_prompt_bytes = final_prompt.len(),
            "media_pipeline: stage 1 (flux-2-max) starting"
        );

        let resp = self
            .flash_image_provider
            .generate_image_with_input(&FlashImageWithInputRequest {
                prompt: final_prompt,
                input_image: input.character_plate_url.clone(),
                width: STAGE1_FLUX_WIDTH,
                height: STAGE1_FLUX_HEIGHT,
                seed: Some(seed),
                output_format: "png".into(),
            })
            .await
            .map_err(|e| {
                tracing::error!(
                    simulation_id = %input.simulation_id,
                    clip_tag = %input.clip_tag,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    error = ?e,
                    "media_pipeline: stage 1 provider call failed"
                );
                e
            })?;
        if resp.image_bytes.is_empty() {
            return Err(anyhow!("empty image data returned"));
        }

        // Dim sanity: Flux returns whatever it rendered; if it drifted off the
        // 16:9 pair we asked for, downstream Runway ratio validation may fail.
        // Cheap decode just to read dimensions.
        if let Err(e) = assert_stage1_dims(&resp.image_bytes) {
            tracing::warn!(
                simulation_id = %input.simulation_id,
                clip_tag = %input.clip_tag,
                error = %e,
                "media_pipeline: stage 1 dim assertion failed"
            );
        }

        tracing::info!(
            simulation_id = %input.simulation_id,
            clip_tag = %input.clip_tag,
            bytes = resp.image_bytes.len(),
            mime = %resp.mime_type,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "media_pipeline: stage 1 image generated"
        );
        let storage_path = self.stage_path(input, "scene_base.png");
        self.object_store
            .upload(
                &self.media_bucket,
                &storage_path,
                resp.image_bytes.clone(),
                &resp.mime_type,
            )
            .await?;
        let signed_url = self
            .object_store
            .get_external_signed_url(&self.media_bucket, &storage_path, Duration::from_secs(3600))
            .await?;
        tracing::info!(
            simulation_id = %input.simulation_id,
            clip_tag = %input.clip_tag,
            storage_path = %storage_path,
            total_elapsed_ms = started.elapsed().as_millis() as u64,
            "media_pipeline: stage 1 complete"
        );
        Ok(PipelineArtifact {
            data: resp.image_bytes,
            mime_type: resp.mime_type,
            storage_path,
            storage_url: signed_url,
        })
    }

    async fn execute_stage2(
        &self,
        input: &PipelineInput,
        scene: &PipelineArtifact,
        seed: i64,
    ) -> Result<PipelineArtifact> {
        let started = std::time::Instant::now();
        let dur = if input.duration_secs < 2 {
            10
        } else {
            input.duration_secs
        };

        // Note: we intentionally do NOT pass `character_plate_url` as a
        // referenceImage here. Stage 1 (Flux) has already composed the
        // character into the scene; sending the plate as a second identity
        // signal caused subtle face drift as Runway blended two slightly
        // different faces. gen4.5 anchors on the scene base alone.

        // Motion prompt is passed through unchanged. Provider-specific
        // finishing (no-other-subjects suffix, byte-budget trimming for
        // Runway's ~1000 UTF-16 cap) happens inside `RunwayProvider` now.
        // Veo/Seedance will apply their own finishing when those providers land.
        tracing::info!(
            simulation_id = %input.simulation_id,
            clip_tag = %input.clip_tag,
            duration_secs = dur,
            input_scene_bytes = scene.data.len(),
            seed,
            motion_prompt_preview = %truncate_for_log(&input.motion_prompt, 200),
            "media_pipeline: stage 2 starting"
        );
        let model = if self.video_model.is_empty() {
            "gen4.5".to_string()
        } else {
            self.video_model.clone()
        };
        let resp = self
            .video_provider
            .generate_video(&VideoRequest {
                prompt: input.motion_prompt.clone(),
                model,
                input_image_url: scene.storage_url.clone(),
                reference_images: vec![],
                aspect_ratio: DEFAULT_PIPELINE_VIDEO_ASPECT_RATIO.into(),
                duration_secs: dur,
                input_video_url: String::new(),
                seed: Some(seed),
                // Structured fields — Veo-family providers compose them into
                // a 5-part prompt; gen4.x ignores and uses `prompt` only.
                scene_prompt: input.scene_prompt.clone(),
                edit_prompt: input.edit_prompt.clone(),
                narration_text: input.narration_text.clone(),
                ..Default::default()
            })
            .await
            .map_err(|e| {
                tracing::error!(
                    simulation_id = %input.simulation_id,
                    clip_tag = %input.clip_tag,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    error = ?e,
                    "media_pipeline: stage 2 provider call failed"
                );
                e
            })?;
        if resp.video_data.is_empty() {
            return Err(anyhow!("empty video data returned"));
        }
        if !is_valid_mp4_header(&resp.video_data) {
            return Err(anyhow!("invalid MP4 header in motion video"));
        }
        tracing::info!(
            simulation_id = %input.simulation_id,
            clip_tag = %input.clip_tag,
            bytes = resp.video_data.len(),
            mime = %resp.mime_type,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "media_pipeline: stage 2 video generated"
        );
        let storage_path = self.stage_path(input, "motion.mp4");
        self.object_store
            .upload(
                &self.media_bucket,
                &storage_path,
                resp.video_data.clone(),
                &resp.mime_type,
            )
            .await?;
        let signed_url = self
            .object_store
            .get_external_signed_url(&self.media_bucket, &storage_path, Duration::from_secs(3600))
            .await?;
        tracing::info!(
            simulation_id = %input.simulation_id,
            clip_tag = %input.clip_tag,
            storage_path = %storage_path,
            total_elapsed_ms = started.elapsed().as_millis() as u64,
            "media_pipeline: stage 2 complete"
        );
        Ok(PipelineArtifact {
            data: resp.video_data,
            mime_type: resp.mime_type,
            storage_path,
            storage_url: signed_url,
        })
    }

    fn stage_path(&self, input: &PipelineInput, filename: &str) -> String {
        if !input.clip_tag.is_empty() {
            format!(
                "generated-videos/{}/{}/{}/{}",
                input.user_id, input.simulation_id, input.clip_tag, filename
            )
        } else {
            format!(
                "generated-videos/{}/{}/{}",
                input.user_id, input.simulation_id, filename
            )
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PipelineInput {
    pub simulation_id: Uuid,
    pub user_id: Uuid,
    pub scene_prompt: String,
    pub motion_prompt: String,
    pub edit_prompt: String,
    pub profile_photo_url: String,
    pub character_plate_url: String,
    pub duration_secs: u32,
    pub clip_tag: String,
    /// Optional. When unset, `execute` generates a per-run seed and reuses it
    /// across all three stages for character-identity consistency within a clip.
    /// Callers SHOULD pass a `simulation_id`-derived seed here so all phases of
    /// a single simulation share one seed and faces don't drift across clips.
    pub seed: Option<i64>,
    /// Short prose description of the subject (age, profession, bearing,
    /// wardrobe/aesthetic hints) injected into the Stage 1 Flux prompt as
    /// `Subject context: ...`. Empty string means "no extra context" — the
    /// pipeline still works, it just doesn't carry user-state into the frame.
    /// See `crate::prompts::build_user_state_block`.
    pub user_state_block: String,
    /// Narration line emitted by the scenario planner per phase. Runway
    /// gen4.x ignores it; Veo-family providers weave it as spoken dialogue
    /// in the assembled prompt. Empty when the planner emitted nothing.
    pub narration_text: String,
}

#[derive(Debug, Clone, Default)]
pub struct PipelineOutput {
    pub scene_image: PipelineArtifact,
    pub motion_video: PipelineArtifact,
    pub final_video: PipelineArtifact,
}

#[derive(Debug, Clone, Default)]
pub struct PipelineArtifact {
    pub data: Vec<u8>,
    pub mime_type: String,
    pub storage_path: String,
    pub storage_url: String,
}

/// Truncate a string to `max` bytes on a char boundary, for log fields.
fn truncate_for_log(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…[+{} chars]", &s[..end], s.len() - end)
}

/// Validates that the bytes begin with a plausible MP4 "ftyp" box.
/// Matches Go's isValidMP4Header logic.
fn is_valid_mp4_header(data: &[u8]) -> bool {
    if data.len() < 12 {
        return false;
    }
    // Box at offset 4..8 should be "ftyp".
    &data[4..8] == b"ftyp"
}

/// Confirm Stage 1's Flux output matches the 16:9 Flux-safe pair we requested.
/// BFL silently rounds dims that aren't multiples of 32 — this catches drift
/// before the image is uploaded and handed to Runway's strict-enum ratio check.
fn assert_stage1_dims(data: &[u8]) -> anyhow::Result<()> {
    use image::ImageReader;
    use std::io::Cursor;
    let reader = ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .map_err(|e| anyhow!("stage1 dim probe: {}", e))?;
    let (w, h) = reader
        .into_dimensions()
        .map_err(|e| anyhow!("stage1 dim probe: {}", e))?;
    if w != STAGE1_FLUX_WIDTH || h != STAGE1_FLUX_HEIGHT {
        return Err(anyhow!(
            "stage1 dims off: expected {}x{}, got {}x{}",
            STAGE1_FLUX_WIDTH,
            STAGE1_FLUX_HEIGHT,
            w,
            h
        ));
    }
    Ok(())
}

/// Resize an image to target max megapixels, return JPEG bytes.
pub fn resize_to_max_megapixels(
    data: &[u8],
    content_type: &str,
    max_mp: f32,
    quality: u8,
) -> anyhow::Result<(Vec<u8>, String)> {
    use image::codecs::jpeg::JpegEncoder;
    use image::ImageReader;
    use std::io::Cursor;

    let img = ImageReader::new(Cursor::new(data))
        .with_guessed_format()?
        .decode()?;
    let (w, h) = (img.width(), img.height());
    let total = w as f32 * h as f32;
    let max_pixels = max_mp * 1_000_000.0;
    let resized = if total > max_pixels {
        let ratio = (max_pixels / total).sqrt();
        let new_w = (w as f32 * ratio).round() as u32;
        let new_h = (h as f32 * ratio).round() as u32;
        img.resize_exact(new_w, new_h, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    let rgb = resized.to_rgb8();
    let mut out = Cursor::new(Vec::new());
    let mut encoder = JpegEncoder::new_with_quality(&mut out, quality);
    encoder.encode(
        rgb.as_raw(),
        rgb.width(),
        rgb.height(),
        image::ExtendedColorType::Rgb8,
    )?;
    let _ = content_type;
    Ok((out.into_inner(), "image/jpeg".into()))
}
