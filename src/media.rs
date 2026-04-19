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

/// Wrap for the scene prompt handed to Flux. The prefix carries the
/// subject-placement directive (Flux must treat the reference as "this
/// person in the scene", not "reproduce the reference"); the suffix
/// carries cinematic camera/lens/grade language so Runway's motion stage
/// inherits the composition instead of being asked to reinvent it.
const PIPELINE_SCENE_PREFIX: &str = "Place the subject from the reference image into the scene described. Preserve their exact facial features, hair, skin tone, and build — this is the same person. The subject faces the camera directly (forward-facing, head and shoulders squared to the lens); do not render profile or back views. Scene: ";
const PIPELINE_SCENE_SUFFIX: &str = " Cinematic photograph, shot on Sony A7 IV with a 35mm f/1.4 lens, available light, 4K ultra-detailed. Natural skin with visible pore texture and subtle asymmetry — reject smooth \"AI face\" look. Anatomically correct hands with five visible fingers. Documentary prestige-film aesthetic, Kodak Portra 800 palette. Pure photograph; no illustration or stylization.";
const PIPELINE_SCENE_CONSTRAINTS: &str = "\n\nConstraints: no visible text, typography, captions, signage, logos, or device screens/UI. Horizontal 16:9 framing. Only the subject appears; no other people in the frame.";

/// Suffix appended to the motion prompt handed to Runway gen4.5. Tells the
/// model not to invent additional people during the animation — the scene
/// base already establishes who is in the frame. Runway's `promptText`
/// accepts ~1000 UTF-16 code units; keep the total under MOTION_PROMPT_MAX_BYTES.
const PIPELINE_MOTION_SUFFIX: &str = " No other people appear in the frame; do not introduce additional subjects or extras at any point in the clip.";
const PIPELINE_MOTION_PROMPT_MAX_BYTES: usize = 950;
/// Leaves headroom for BFL Flux prompt limits. Flux is more generous than
/// Runway's 1000-UTF-16 cap, but staying under 2KB avoids surprises.
const PIPELINE_PROMPT_MAX_BYTES: usize = 2000;

/// Trim `s` to at most `max_bytes`, backing up to the nearest UTF-8 char
/// boundary. Never panics on multi-byte chars. Mirrors the helper in
/// src/jobs/worker.rs — intentional duplicate until a shared utils module exists.
fn truncate_to_byte_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

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
        let seed: i64 = input
            .seed
            .unwrap_or_else(|| rand::random::<u32>() as i64);
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
        let scene = self.execute_stage1(&input, seed).await.context("stage 1 (base)")?;
        let motion = self.execute_stage2(&input, &scene, seed).await.context("stage 2 (motion)")?;

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

        // Size-budget the variable scene text so prefix + suffix + constraints
        // always fit under PIPELINE_PROMPT_MAX_BYTES.
        let fixed_len = PIPELINE_SCENE_PREFIX.len()
            + PIPELINE_SCENE_SUFFIX.len()
            + PIPELINE_SCENE_CONSTRAINTS.len();
        let scene_budget = PIPELINE_PROMPT_MAX_BYTES.saturating_sub(fixed_len);
        let scene_trimmed = truncate_to_byte_boundary(&input.scene_prompt, scene_budget);
        if scene_trimmed.len() < input.scene_prompt.len() {
            tracing::debug!(
                simulation_id = %input.simulation_id,
                clip_tag = %input.clip_tag,
                original_len = input.scene_prompt.len(),
                trimmed_len = scene_trimmed.len(),
                "media_pipeline: stage 1 trimmed scene prompt to fit budget"
            );
        }
        let final_prompt = format!(
            "{}{}{}{}",
            PIPELINE_SCENE_PREFIX, scene_trimmed, PIPELINE_SCENE_SUFFIX, PIPELINE_SCENE_CONSTRAINTS
        );

        tracing::info!(
            simulation_id = %input.simulation_id,
            clip_tag = %input.clip_tag,
            width = STAGE1_FLUX_WIDTH,
            height = STAGE1_FLUX_HEIGHT,
            seed,
            scene_prompt_preview = %truncate_for_log(&input.scene_prompt, 200),
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
            .upload(&self.media_bucket, &storage_path, resp.image_bytes.clone(), &resp.mime_type)
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
        let dur = if input.duration_secs < 2 { 10 } else { input.duration_secs };

        // Note: we intentionally do NOT pass `character_plate_url` as a
        // referenceImage here. Stage 1 (Flux) has already composed the
        // character into the scene; sending the plate as a second identity
        // signal caused subtle face drift as Runway blended two slightly
        // different faces. gen4.5 anchors on the scene base alone.

        // Wrap the motion prompt with a no-other-subjects constraint, staying
        // under Runway's ~1000 UTF-16 cap on promptText.
        let motion_budget = PIPELINE_MOTION_PROMPT_MAX_BYTES.saturating_sub(PIPELINE_MOTION_SUFFIX.len());
        let motion_trimmed = truncate_to_byte_boundary(&input.motion_prompt, motion_budget);
        let final_motion_prompt = format!("{}{}", motion_trimmed, PIPELINE_MOTION_SUFFIX);

        tracing::info!(
            simulation_id = %input.simulation_id,
            clip_tag = %input.clip_tag,
            duration_secs = dur,
            input_scene_bytes = scene.data.len(),
            seed,
            motion_prompt_preview = %truncate_for_log(&final_motion_prompt, 200),
            "media_pipeline: stage 2 (gen4.5) starting"
        );
        let model = if self.video_model.is_empty() {
            "gen4.5".to_string()
        } else {
            self.video_model.clone()
        };
        let resp = self
            .video_provider
            .generate_video(&VideoRequest {
                prompt: final_motion_prompt,
                model,
                input_image_url: scene.storage_url.clone(),
                reference_images: vec![],
                aspect_ratio: DEFAULT_PIPELINE_VIDEO_ASPECT_RATIO.into(),
                duration_secs: dur,
                input_video_url: String::new(),
                seed: Some(seed),
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
            .upload(&self.media_bucket, &storage_path, resp.video_data.clone(), &resp.mime_type)
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
    pub seed: Option<i64>,
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
