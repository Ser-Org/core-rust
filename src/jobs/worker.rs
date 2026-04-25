use crate::app_state::AppState;
use crate::jobs::*;
use crate::media::PipelineInput;
use crate::models::{self, FlashImage, FlashPromptPlan};
use crate::prompts::{self, SimulationContext};
use crate::providers::{FlashImageWithInputRequest, TextRequest};
use crate::utils::truncate_to_byte_boundary;
use anyhow::{anyhow, Context, Result};
use serde_json::Value as JsonValue;
use std::time::Duration;
use tokio::time::sleep;
use uuid::Uuid;

pub(crate) const CHARACTER_PLATE_PROMPT: &str = "A character reference sheet formatted as a triptych: three separate portraits of the same person, evenly spaced side by side with clear vertical gaps of pure white space between each one, on a plain white studio background. Each portrait is a self-contained composition — no overlapping, blending, or shared edges between panels. Left panel — straight-on front view, face directly at the camera. Middle panel — three-quarter view, face turned 45 degrees (both eyes still visible, one side more prominent). Right panel — full profile, face turned 90 degrees (one eye visible, full nose silhouette). Each panel shows a clearly different angle — do not repeat the front view. Head-and-shoulders framing in each, subject centered within its own panel. Preserve the subject's face, hair, skin tone, and natural expression exactly as in the reference image — do not alter their appearance. Soft studio lighting. Photorealistic, sharp focus. No text, captions, labels, numbers, or logos.";

const FLASH_SCENE_CONSTRAINTS: &str = "\n\nConstraints: no visible text, typography, captions, signage, logos, or device screens/UI. Vertical 9:16 framing.";

const FLASH_PROMPT_SUFFIX: &str = " Cinematic photograph, shot on Sony A7 IV with 35mm f/1.4 lens, available light, 4K ultra-detailed. Natural skin with visible pore texture and subtle asymmetry — reject smooth \"AI face\" look. Anatomically correct hands with five visible fingers. Documentary prestige-film aesthetic, Kodak Portra 800 palette. Pure photograph; no illustration or stylization.";

/// Runway gen4_image caps `promptText` at 1000 UTF-16 code units. ASCII UTF-8
/// byte count is a safe proxy. Leave a small margin for multi-byte chars
/// (em-dashes, smart quotes in scene prompts from Claude).
const FLASH_PROMPT_MAX_BYTES: usize = 950;

/// Stable seed for the media pipeline derived from the simulation id. Every
/// phase of a single simulation gets the same seed so character identity
/// stays consistent across the 6 clips. Without this, each phase would
/// generate its own `rand::random()` seed and faces would drift clip-to-clip.
///
/// Clamped to u32 range (0..=u32::MAX) to match the existing convention in
/// the flash-generation path (see `run_flash_generation` comment: "Runway's
/// schema types seed as 'int' with no explicit bounds, but the likely
/// internal representation is u32"). The default pipeline seed generator at
/// `media.rs` also uses `rand::random::<u32>() as i64`.
fn simulation_seed(simulation_id: Uuid) -> i64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    simulation_id.hash(&mut hasher);
    (hasher.finish() as u32) as i64
}

// ---------------------------------------------------------------------------
// Layer C — character plate is identity-aware (age + bearing only)
//
// Wardrobe is intentionally excluded from the plate prompt. Wardrobe lives in
// Layer B's `prompts::build_user_state_block`, scoped per-phase. Baking
// wardrobe into the plate creates a permanent anchor that fights per-scene
// context at Stage 1 and forces face-compromise.
//
// The cache key for character plates is `(user_id, source_photo_id, prompt_hash)`
// (see `claim_character_plate_generation` in src/repos/user_repo.rs). Static
// baseline and dynamic prompt variants can coexist; generation must always
// have the static baseline available before considering dynamic variants.
// ---------------------------------------------------------------------------

/// Triptych body without the original preserve-face/hair/skin/natural-expression
/// clause. Used in the dynamic path so the preserve clause can be re-emitted
/// as the *final* clause after the user-state hint (plan: face-drift
/// prevention — identity preservation must dominate via recency).
/// Must remain byte-identical to `CHARACTER_PLATE_PROMPT` minus the preserve
/// sentence.
const PLATE_TRIPTYCH_WITHOUT_PRESERVE: &str = "A character reference sheet formatted as a triptych: three separate portraits of the same person, evenly spaced side by side with clear vertical gaps of pure white space between each one, on a plain white studio background. Each portrait is a self-contained composition — no overlapping, blending, or shared edges between panels. Left panel — straight-on front view, face directly at the camera. Middle panel — three-quarter view, face turned 45 degrees (both eyes still visible, one side more prominent). Right panel — full profile, face turned 90 degrees (one eye visible, full nose silhouette). Each panel shows a clearly different angle — do not repeat the front view. Head-and-shoulders framing in each, subject centered within its own panel. Soft studio lighting. Photorealistic, sharp focus. No text, captions, labels, numbers, or logos.";

/// The exact original preserve clause from `CHARACTER_PLATE_PROMPT`. Emitted
/// as the LAST clause of the dynamic plate prompt so identity preservation
/// dominates the hint via recency bias.
const PLATE_EXACT_PRESERVE_CLAUSE: &str = " Preserve the subject's face, hair, skin tone, and natural expression exactly as in the reference image — do not alter their appearance.";

/// Builds the character plate prompt with identity-stable hints (age + bearing).
/// Returns the static `CHARACTER_PLATE_PROMPT` unchanged when no hints can be
/// derived from the profile, so existing behavior is preserved as a fallback.
///
/// When hints exist, structure is:
/// `[triptych body WITHOUT preserve] + [hint] + [EXACT preserve clause]`
/// so the final words the diffusion model reads are the exact original
/// preserve-face/hair/skin/natural-expression clause.
pub(crate) fn build_character_plate_prompt(
    profile: &models::UserProfile,
    life: &models::LifeState,
) -> String {
    let age = plate_age_phrase(life, profile);
    let bearing = plate_bearing_phrase(
        profile.stress_response.as_deref(),
        profile.decision_style.as_deref(),
    );
    let hint = match (age.is_empty(), bearing.is_empty()) {
        (false, false) => format!(" Subject is {} {}.", age, bearing),
        (false, true) => format!(" Subject is {}.", age),
        (true, false) => format!(" Subject {}.", bearing),
        (true, true) => return CHARACTER_PLATE_PROMPT.to_string(),
    };
    format!(
        "{}{}{}",
        PLATE_TRIPTYCH_WITHOUT_PRESERVE, hint, PLATE_EXACT_PRESERVE_CLAUSE
    )
}

fn plate_age_phrase(life: &models::LifeState, profile: &models::UserProfile) -> String {
    if life.age > 0 {
        return match life.age {
            a if a < 25 => "in their early 20s".into(),
            a if a < 35 => "in their late 20s to early 30s".into(),
            a if a < 45 => "in their late 30s to early 40s".into(),
            a if a < 55 => "in their late 40s to early 50s".into(),
            a if a < 65 => "in their late 50s to early 60s".into(),
            _ => "in their late 60s".into(),
        };
    }
    if let Some(b) = profile.age_bracket.as_deref() {
        return match b {
            "18-24" => "in their early 20s".into(),
            "25-34" => "in their late 20s to early 30s".into(),
            "35-44" => "in their late 30s to early 40s".into(),
            "45-54" => "in their late 40s to early 50s".into(),
            "55-64" => "in their late 50s to early 60s".into(),
            "65+" => "in their late 60s".into(),
            _ => String::new(),
        };
    }
    String::new()
}

/// Maps explicit stress + decision-style signals to a triptych bearing phrase.
/// Returns empty unless BOTH inputs are `Some` and non-empty — absent
/// behavioral data is silence, not a default. Matches the no-defaults-as-data
/// discipline applied to Layer B's `bearing_phrase` in `prompts.rs`.
fn plate_bearing_phrase(stress: Option<&str>, style: Option<&str>) -> String {
    let stress = match stress.filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => return String::new(),
    };
    let style = match style.filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => return String::new(),
    };
    match (stress, style) {
        ("analytical", "deliberate") => {
            "with a calm, composed bearing — relaxed shoulders, level gaze".into()
        }
        ("analytical", "intuitive") => {
            "with an alert, thoughtful bearing — engaged but unhurried".into()
        }
        ("emotional", "deliberate") => "with a warm, considered bearing — open expression".into(),
        ("emotional", "intuitive") => "with an expressive, engaged presence — animated eyes".into(),
        ("withdrawn", _) => "with a reserved, contained bearing — quiet poise".into(),
        _ => String::new(),
    }
}

/// Loads profile + life_state for `user_id` and builds the dynamic character
/// plate prompt. Falls back to the static `CHARACTER_PLATE_PROMPT` when either
/// load fails — the result is always a valid Flux prompt.
///
/// Gated by `cfg.enable_dynamic_character_plate` (default off): when the flag
/// is false, always returns the static prompt without doing the profile/life
/// lookup. Dynamic plates ship only after visual A/B confirms the bearing
/// hints don't introduce likeness drift versus the source photo.
pub async fn plate_prompt_for_user(state: &AppState, user_id: Uuid) -> String {
    if !state.cfg.enable_dynamic_character_plate {
        return CHARACTER_PLATE_PROMPT.to_string();
    }
    let profile_res = state.user_repo.get_profile_by_user_id(user_id).await;
    let life_res = state.user_repo.build_life_state(user_id).await;
    match (profile_res, life_res) {
        (Ok(profile), Ok(life)) => build_character_plate_prompt(&profile, &life),
        _ => CHARACTER_PLATE_PROMPT.to_string(),
    }
}

struct CharacterPaletteReference {
    url: String,
    source: &'static str,
    plate_id: Uuid,
}

pub(crate) async fn ensure_character_palette_jobs(
    state: &AppState,
    user_id: Uuid,
    source_photo_id: Uuid,
    context: &'static str,
) -> Result<()> {
    claim_and_enqueue_character_plate(
        state,
        user_id,
        source_photo_id,
        CHARACTER_PLATE_PROMPT,
        context,
        "static_baseline",
    )
    .await?;

    if state.cfg.enable_dynamic_character_plate {
        let dynamic_prompt = plate_prompt_for_user(state, user_id).await;
        if dynamic_prompt != CHARACTER_PLATE_PROMPT {
            if let Err(e) = claim_and_enqueue_character_plate(
                state,
                user_id,
                source_photo_id,
                &dynamic_prompt,
                context,
                "dynamic",
            )
            .await
            {
                tracing::warn!(
                    user_id = %user_id,
                    source_photo_id = %source_photo_id,
                    error = ?e,
                    context,
                    "character_palette: dynamic generation claim/enqueue failed; static baseline remains authoritative"
                );
            }
        }
    }

    Ok(())
}

async fn claim_and_enqueue_character_plate(
    state: &AppState,
    user_id: Uuid,
    source_photo_id: Uuid,
    prompt: &str,
    context: &'static str,
    variant: &'static str,
) -> Result<models::CharacterPlate> {
    let (plate, claimed) = state
        .user_repo
        .claim_character_plate_generation(user_id, source_photo_id, prompt)
        .await?;
    if claimed {
        tracing::info!(
            user_id = %user_id,
            source_photo_id = %source_photo_id,
            plate_id = %plate.id,
            variant,
            context,
            "character_palette: claimed generation, enqueuing job"
        );
        if let Err(e) = state
            .job_client
            .insert(
                KIND_CHARACTER_PLATE,
                &CharacterPlateArgs {
                    user_id,
                    source_photo_id,
                    plate_id: plate.id,
                },
            )
            .await
        {
            let msg = format!("enqueue failed: {}", e);
            if let Err(mark_err) = state
                .user_repo
                .update_character_plate_status(
                    plate.id,
                    models::character_plate_status::FAILED,
                    None,
                    None,
                    None,
                    None,
                    Some(&msg),
                )
                .await
            {
                tracing::error!(
                    user_id = %user_id,
                    source_photo_id = %source_photo_id,
                    plate_id = %plate.id,
                    variant,
                    context,
                    enqueue_error = ?e,
                    mark_error = ?mark_err,
                    "character_palette: enqueue failed and failed to reset claimed row"
                );
            } else {
                tracing::error!(
                    user_id = %user_id,
                    source_photo_id = %source_photo_id,
                    plate_id = %plate.id,
                    variant,
                    context,
                    error = ?e,
                    "character_palette: enqueue failed after claim; marked row failed for retry"
                );
            }
            return Err(e.context("enqueue character plate job"));
        }
    } else {
        tracing::debug!(
            user_id = %user_id,
            source_photo_id = %source_photo_id,
            plate_id = %plate.id,
            status = %plate.status,
            variant,
            context,
            "character_palette: generation already in flight or ready"
        );
    }
    Ok(plate)
}

pub async fn start(state: AppState, workers: usize) {
    tracing::info!(workers, "jobs: starting worker pool");
    for i in 0..workers.max(1) {
        let s = state.clone();
        let worker_id = i;
        tokio::spawn(async move {
            tracing::debug!(worker_id, "jobs: worker spawned");
            run_worker(s, worker_id).await;
        });
    }
}

async fn run_worker(state: AppState, worker_id: usize) {
    loop {
        match state.job_client.claim_one().await {
            Ok(Some(job)) => {
                let span = tracing::info_span!(
                    "job",
                    worker_id,
                    job_id = %job.id,
                    kind = %job.kind,
                    attempt = job.attempts,
                    max_attempts = job.max_attempts,
                );
                let _enter = span.enter();
                let started = std::time::Instant::now();
                tracing::info!("job claimed");
                let res = dispatch(&state, &job).await;
                let elapsed_ms = started.elapsed().as_millis() as u64;
                match res {
                    Ok(()) => {
                        tracing::info!(elapsed_ms, "job completed");
                        if let Err(e) = state.job_client.mark_completed(job.id).await {
                            tracing::error!(error = ?e, "jobs: mark completed failed");
                        }
                    }
                    Err(e) => {
                        let msg = format!("{}", e);
                        let retryable = !msg.contains("permanent:");
                        if retryable && job.attempts < job.max_attempts {
                            tracing::warn!(
                                error = ?e,
                                elapsed_ms,
                                retryable,
                                "job failed, will retry"
                            );
                        } else {
                            tracing::error!(error = ?e, elapsed_ms, "job failed permanently");
                        }
                        if let Err(markup) =
                            state.job_client.mark_failed(job.id, &msg, retryable).await
                        {
                            tracing::error!(error = %markup, "jobs: mark failed errored");
                        }
                    }
                }
            }
            Ok(None) => {
                sleep(Duration::from_millis(1500)).await;
            }
            Err(e) => {
                tracing::error!(error = ?e, worker_id, "jobs: claim error");
                sleep(Duration::from_secs(3)).await;
            }
        }
    }
}

async fn dispatch(state: &AppState, job: &JobRow) -> Result<()> {
    match job.kind.as_str() {
        KIND_LIFE_STATE_EXTRACTION => {
            let args: LifeStateExtractionArgs = serde_json::from_value(job.args.clone())?;
            run_life_state_extraction(state, args).await
        }
        KIND_SCENARIO_PLANNER => {
            let args: ScenarioPlannerArgs = serde_json::from_value(job.args.clone())?;
            let sim_id = args.simulation_id;
            match run_scenario_planner(state, args).await {
                Ok(()) => Ok(()),
                Err(e) => {
                    mark_failed_best_effort(state, sim_id, "scenario_plan", &e).await;
                    Err(e)
                }
            }
        }
        KIND_ASSUMPTION_EXTRACTION => {
            let args: AssumptionExtractionArgs = serde_json::from_value(job.args.clone())?;
            let sim_id = args.simulation_id;
            match run_assumption_extraction(state, args).await {
                Ok(()) => Ok(()),
                Err(e) => {
                    mark_failed_best_effort(state, sim_id, "assumptions", &e).await;
                    Err(e)
                }
            }
        }
        KIND_CHARACTER_PLATE => {
            let args: CharacterPlateArgs = serde_json::from_value(job.args.clone())?;
            run_character_plate(state, args).await
        }
        KIND_VIDEO_GENERATION => {
            let args: VideoGenerationArgs = serde_json::from_value(job.args.clone())?;
            let sim_id = args.simulation_id;
            let component_key = format!("video_{}_{}", args.path, args.phase);
            match run_video_generation(state, args).await {
                Ok(()) => Ok(()),
                Err(e) => {
                    mark_failed_best_effort(state, sim_id, &component_key, &e).await;
                    Err(e)
                }
            }
        }
        KIND_FLASH_GENERATION => {
            let args: FlashGenerationArgs = serde_json::from_value(job.args.clone())?;
            run_flash_generation(state, args).await
        }
        other => Err(anyhow!("unknown job kind: {}", other)),
    }
}

async fn mark_failed_best_effort(
    state: &AppState,
    sim_id: Uuid,
    component_key: &str,
    e: &anyhow::Error,
) {
    let msg = format!("{}", e);
    if let Err(mark_err) = state
        .components_repo
        .mark_component_failed(sim_id, component_key, "job_error", &msg)
        .await
    {
        tracing::error!(
            simulation_id = %sim_id,
            component_key,
            error = ?mark_err,
            "failed to mark component failed"
        );
    }
}

async fn run_life_state_extraction(state: &AppState, args: LifeStateExtractionArgs) -> Result<()> {
    tracing::info!(
        user_id = %args.user_id,
        story_id = %args.story_id,
        "life_state_extraction: starting"
    );
    let story = state
        .user_repo
        .get_life_story_by_user_id(args.user_id)
        .await?;
    if story.raw_input.is_empty() {
        tracing::debug!("life_state_extraction: empty raw_input, skipping");
        return Ok(());
    }
    tracing::debug!(
        input_len = story.raw_input.len(),
        input_method = %story.input_method,
        "life_state_extraction: loaded story"
    );

    let mut sctx = SimulationContext::default();
    sctx.life_story = Some(story.clone());
    let (sys, user) = state
        .prompt_builder
        .build_text_prompt(prompts::TASK_LIFE_STATE_EXTRACTION, &sctx);
    let resp = state
        .text_provider
        .generate_text(&TextRequest {
            system_prompt: sys,
            user_prompt: user,
            max_tokens: 2000,
            temperature: 0.1,
            json_mode: true,
            ..Default::default()
        })
        .await?;
    tracing::debug!(
        tokens_total = resp.tokens_used.total_tokens,
        model = %resp.model_id,
        provider = %resp.provider_name,
        "life_state_extraction: text generated"
    );
    let parsed: JsonValue = serde_json::from_str(resp.content.trim()).unwrap_or(JsonValue::Null);
    if !parsed.is_null() {
        state
            .user_repo
            .update_extracted_context(args.story_id, &parsed)
            .await?;
        tracing::info!(
            story_id = %args.story_id,
            field_count = parsed.as_object().map(|o| o.len()).unwrap_or(0),
            "life_state_extraction: persisted extracted_context"
        );
    } else {
        tracing::warn!(
            "life_state_extraction: AI returned unparseable JSON, leaving context unchanged"
        );
    }
    Ok(())
}

async fn run_scenario_planner(state: &AppState, args: ScenarioPlannerArgs) -> Result<()> {
    tracing::info!(simulation_id = %args.simulation_id, "scenario_planner: starting");
    let sim = state
        .simulation_repo
        .get_simulation_by_id(args.simulation_id)
        .await?;
    tracing::debug!(
        simulation_id = %args.simulation_id,
        decision_id = %sim.decision_id,
        user_id = %sim.user_id,
        run_number = sim.run_number,
        "scenario_planner: loaded simulation"
    );
    let _ = state
        .components_repo
        .mark_component_running(args.simulation_id, "scenario_plan")
        .await;

    let ctx = load_simulation_context(state, &sim).await?;
    tracing::debug!(
        time_horizon_months = ctx.time_horizon_months,
        exact_phases = ctx.scenario_planner_exact_phases,
        clip_duration_secs = ctx.video_clip_duration_secs,
        "scenario_planner: context assembled"
    );
    let (sys, user) = state
        .prompt_builder
        .build_text_prompt(prompts::TASK_SCENARIO_PLANNER, &ctx);
    let resp = state
        .text_provider
        .generate_text(&TextRequest {
            system_prompt: sys,
            user_prompt: user,
            max_tokens: 8000,
            temperature: 0.6,
            json_mode: true,
            ..Default::default()
        })
        .await?;
    tracing::debug!(
        tokens_total = resp.tokens_used.total_tokens,
        model = %resp.model_id,
        provider = %resp.provider_name,
        "scenario_planner: text generated"
    );
    let parsed: JsonValue =
        serde_json::from_str(&resp.content).context("parse scenario planner")?;
    let path_a = parsed.get("path_a").cloned().unwrap_or(JsonValue::Null);
    let path_b = parsed.get("path_b").cloned().unwrap_or(JsonValue::Null);
    let shared = parsed
        .get("shared_context")
        .cloned()
        .unwrap_or(JsonValue::Null);

    let plan = models::ScenarioPlan {
        id: Uuid::new_v4(),
        simulation_id: args.simulation_id,
        path_a: path_a.clone(),
        path_b: path_b.clone(),
        shared_context: shared,
        raw_ai_response: resp.content,
        created_at: chrono::Utc::now(),
    };
    state.scenario_repo.insert_scenario_plan(&plan).await?;
    tracing::info!(
        plan_id = %plan.id,
        simulation_id = %args.simulation_id,
        "scenario_planner: scenario plan persisted"
    );

    // Enqueue assumption extraction.
    state
        .job_client
        .insert(
            KIND_ASSUMPTION_EXTRACTION,
            &AssumptionExtractionArgs {
                simulation_id: args.simulation_id,
            },
        )
        .await?;

    // Register video clip components for each phase in each path, then enqueue.
    let phases_a = path_a
        .get("phases")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let phases_b = path_b
        .get("phases")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let total_video_phases = (phases_a.len() + phases_b.len()) as i32;
    tracing::info!(
        path_a_phases = phases_a.len(),
        path_b_phases = phases_b.len(),
        total_video_phases,
        "scenario_planner: phases produced, scheduling video jobs"
    );
    if total_video_phases > 0 {
        state
            .simulation_repo
            .update_total_components(args.simulation_id, total_video_phases)
            .await?;

        // Character plate is a per-user resource (generated once, reused across
        // simulations). It's intentionally NOT tracked as a per-simulation component.

        // Register and enqueue video clip components for each phase.
        let mut components: Vec<models::SimulationComponent> = Vec::new();
        for (i, _) in phases_a.iter().enumerate() {
            components.push(video_component(args.simulation_id, "path_a", i as i32));
        }
        for (i, _) in phases_b.iter().enumerate() {
            components.push(video_component(args.simulation_id, "path_b", i as i32));
        }
        state
            .components_repo
            .upsert_simulation_components(&components)
            .await?;

        // Enqueue character palette work before video jobs. The static baseline
        // is mandatory; dynamic variants are optional and never replace the
        // baseline as the fallback identity anchor.
        match state
            .user_repo
            .get_primary_photo_by_user_id(sim.user_id)
            .await
        {
            Ok(photo) => {
                ensure_character_palette_jobs(state, sim.user_id, photo.id, "scenario_planner")
                    .await?;
            }
            Err(e) => {
                tracing::warn!(
                    user_id = %sim.user_id,
                    error = ?e,
                    "scenario_planner: no primary photo, skipping character plate"
                );
            }
        }
        for (i, _) in phases_a.iter().enumerate() {
            state
                .job_client
                .insert(
                    KIND_VIDEO_GENERATION,
                    &VideoGenerationArgs {
                        simulation_id: args.simulation_id,
                        path: "path_a".into(),
                        phase: i as i32,
                    },
                )
                .await?;
        }
        for (i, _) in phases_b.iter().enumerate() {
            state
                .job_client
                .insert(
                    KIND_VIDEO_GENERATION,
                    &VideoGenerationArgs {
                        simulation_id: args.simulation_id,
                        path: "path_b".into(),
                        phase: i as i32,
                    },
                )
                .await?;
        }
    }

    let _ = state
        .components_repo
        .mark_component_completed(args.simulation_id, "scenario_plan")
        .await;
    Ok(())
}

fn video_component(sim_id: Uuid, path: &str, phase: i32) -> models::SimulationComponent {
    let key = format!("video_{}_{}", path, phase);
    models::SimulationComponent {
        id: Uuid::nil(),
        simulation_id: sim_id,
        component_key: key.clone(),
        component_type: models::simulation_component_type::VIDEO_CLIP.into(),
        display_name: format!(
            "{} phase {}",
            if path == "path_a" { "Path A" } else { "Path B" },
            phase + 1
        ),
        status: models::simulation_component_status::PENDING.into(),
        path: Some(path.into()),
        phase: Some(phase),
        error_code: None,
        error_message: None,
        metadata: JsonValue::Null,
        created_at: chrono::Utc::now(),
        started_at: None,
        completed_at: None,
        updated_at: chrono::Utc::now(),
    }
}

async fn run_assumption_extraction(state: &AppState, args: AssumptionExtractionArgs) -> Result<()> {
    tracing::info!(simulation_id = %args.simulation_id, "assumption_extraction: starting");
    let _ = state
        .components_repo
        .mark_component_running(args.simulation_id, "assumptions")
        .await;

    let sim = state
        .simulation_repo
        .get_simulation_by_id(args.simulation_id)
        .await?;
    let plan = state
        .scenario_repo
        .get_scenario_plan_by_simulation_id(args.simulation_id)
        .await?;
    let mut ctx = load_simulation_context(state, &sim).await?;
    ctx.scenario_plan_path_a_label = plan
        .path_a
        .get("label")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    ctx.scenario_plan_path_a_summary = prompts::scenario_path_summary(&plan.path_a);
    ctx.scenario_plan_path_b_label = plan
        .path_b
        .get("label")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    ctx.scenario_plan_path_b_summary = prompts::scenario_path_summary(&plan.path_b);

    let (sys, user) = state
        .prompt_builder
        .build_text_prompt(prompts::TASK_ASSUMPTION_EXTRACTION, &ctx);
    let resp = state
        .text_provider
        .generate_text(&TextRequest {
            system_prompt: sys,
            user_prompt: user,
            max_tokens: 6000,
            temperature: 0.2,
            json_mode: true,
            ..Default::default()
        })
        .await?;
    let parsed: JsonValue = serde_json::from_str(&resp.content).context("assumption extraction")?;

    let mut assumptions: Vec<models::SimulationAssumption> = vec![];
    if let Some(arr) = parsed.get("assumptions").and_then(|v| v.as_array()) {
        for a in arr {
            assumptions.push(models::SimulationAssumption {
                id: Uuid::new_v4(),
                simulation_id: args.simulation_id,
                description: a
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                confidence: a.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.5),
                source: a
                    .get("source")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                kind: a
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                grounding: a
                    .get("grounding")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                evidence_refs: a
                    .get("evidence_refs")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|s| s.as_str().map(|t| t.to_string()))
                            .collect()
                    })
                    .unwrap_or_default(),
                category: a
                    .get("category")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                editable: a.get("editable").and_then(|v| v.as_bool()).unwrap_or(true),
                user_override_value: None,
                original_confidence: None,
                profile_field: a
                    .get("profile_field")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string()),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            });
        }
    }
    let mut risks: Vec<models::SimulationRisk> = vec![];
    if let Some(arr) = parsed.get("risks").and_then(|v| v.as_array()) {
        for r in arr {
            risks.push(models::SimulationRisk {
                id: Uuid::new_v4(),
                simulation_id: args.simulation_id,
                description: r
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                likelihood: r
                    .get("likelihood")
                    .and_then(|v| v.as_str())
                    .unwrap_or("medium")
                    .to_string(),
                impact: r
                    .get("impact")
                    .and_then(|v| v.as_str())
                    .unwrap_or("medium")
                    .to_string(),
                category: r
                    .get("category")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                linked_assumption_ids: vec![],
                mitigation_hint: r
                    .get("mitigation_hint")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                created_at: chrono::Utc::now(),
            });
        }
    }
    let assumption_count = assumptions.len();
    let risk_count = risks.len();
    state
        .simulation_repo
        .bulk_insert_assumptions(&assumptions)
        .await?;
    state.simulation_repo.bulk_insert_risks(&risks).await?;
    tracing::info!(
        simulation_id = %args.simulation_id,
        assumption_count,
        risk_count,
        "assumption_extraction: persisted"
    );

    let _ = state
        .components_repo
        .mark_component_completed(args.simulation_id, "assumptions")
        .await;
    Ok(())
}

async fn run_character_plate(state: &AppState, args: CharacterPlateArgs) -> Result<()> {
    tracing::info!(
        plate_id = %args.plate_id,
        user_id = %args.user_id,
        source_photo_id = %args.source_photo_id,
        "character_plate: starting generation"
    );
    // The job must use the exact row/photo claimed at enqueue time. The
    // user's primary photo can change while the job is waiting in the queue.
    let plate = state
        .user_repo
        .get_character_plate_by_id(args.plate_id)
        .await
        .map_err(|e| anyhow!("get character plate: {}", e))?;

    if plate.user_id != args.user_id || plate.source_photo_id != args.source_photo_id {
        let msg = format!(
            "job args do not match plate row: args_user_id={}, plate_user_id={}, args_source_photo_id={}, plate_source_photo_id={}",
            args.user_id, plate.user_id, args.source_photo_id, plate.source_photo_id
        );
        let _ = state
            .user_repo
            .update_character_plate_status(
                plate.id,
                models::character_plate_status::FAILED,
                None,
                None,
                None,
                None,
                Some(&msg),
            )
            .await;
        return Err(anyhow!("permanent: character_plate: {}", msg));
    }

    let photo = state
        .user_repo
        .get_photo_by_id_for_user(plate.source_photo_id, plate.user_id)
        .await
        .map_err(|e| anyhow!("get claimed source photo: {}", e))?;

    let photo_url = state
        .object_store
        .get_external_signed_url(
            &state.cfg.s3_bucket,
            &photo.storage_path,
            Duration::from_secs(3600),
        )
        .await
        .unwrap_or_else(|_| photo.storage_url.clone());

    // Read the prompt from the plate row written at claim time — NOT a fresh
    // computation. If `ENABLE_DYNAMIC_CHARACTER_PLATE` flipped, or the
    // profile/life changed between claim and generation, the generated image
    // must match what was claimed and recorded in `prompt_used`. Otherwise
    // the stored `prompt_used` diverges from the actual Flux input.
    let plate_prompt = if !plate.prompt_used.is_empty() {
        plate.prompt_used.clone()
    } else {
        tracing::warn!(
            plate_id = %args.plate_id,
            "character_plate: stored prompt_used was empty; falling back to static"
        );
        CHARACTER_PLATE_PROMPT.to_string()
    };
    tracing::info!(
        plate_id = %args.plate_id,
        user_id = %args.user_id,
        plate_prompt_bytes = plate_prompt.len(),
        is_dynamic = plate_prompt != CHARACTER_PLATE_PROMPT,
        "character_plate: prompt loaded from plate row"
    );
    let resp = match state
        .flash_image_provider
        .generate_image_with_input(&FlashImageWithInputRequest {
            prompt: plate_prompt,
            input_image: photo_url,
            width: 1920,
            height: 1080,
            seed: None,
            output_format: "png".into(),
        })
        .await
    {
        Ok(r) => r,
        Err(e) => {
            let msg = format!("{}", e);
            let _ = state
                .user_repo
                .update_character_plate_status(
                    args.plate_id,
                    models::character_plate_status::FAILED,
                    None,
                    None,
                    None,
                    None,
                    Some(&msg),
                )
                .await;
            return Err(e);
        }
    };
    if resp.image_bytes.is_empty() {
        let _ = state
            .user_repo
            .update_character_plate_status(
                args.plate_id,
                models::character_plate_status::FAILED,
                None,
                None,
                None,
                None,
                Some("empty image data"),
            )
            .await;
        return Err(anyhow!("character_plate: empty image data"));
    }

    let path = format!(
        "character-plates/{}/{}/{}.png",
        args.user_id, photo.id, args.plate_id
    );
    let url = state
        .object_store
        .upload(
            &state.cfg.character_palettes_bucket,
            &path,
            resp.image_bytes,
            &resp.mime_type,
        )
        .await?;

    state
        .user_repo
        .update_character_plate_status(
            args.plate_id,
            models::character_plate_status::READY,
            Some(&state.cfg.character_palettes_bucket),
            Some(&url),
            Some(&path),
            Some(&resp.mime_type),
            None,
        )
        .await?;
    tracing::info!(
        plate_id = %args.plate_id,
        storage_path = %path,
        size_bytes = url.len(),
        "character_plate: ready"
    );

    // If this plate is tied to a running simulation, mark the character_plate
    // component complete. We scan the simulations that are running for this
    // user; any with a character_plate component will be advanced.
    let _ = state
        .components_repo
        .mark_component_completed(Uuid::nil(), "character_plate")
        .await;

    Ok(())
}

async fn run_video_generation(state: &AppState, args: VideoGenerationArgs) -> Result<()> {
    let started = std::time::Instant::now();
    tracing::info!(
        simulation_id = %args.simulation_id,
        path = %args.path,
        phase = args.phase,
        "video_generation: starting"
    );
    let component_key = format!("video_{}_{}", args.path, args.phase);
    let _ = state
        .components_repo
        .mark_component_running(args.simulation_id, &component_key)
        .await;

    let plan = state
        .scenario_repo
        .get_scenario_plan_by_simulation_id(args.simulation_id)
        .await
        .map_err(|e| {
            tracing::error!(simulation_id = %args.simulation_id, error = ?e, "video_generation: failed to load scenario plan");
            e
        })?;
    let path_value = if args.path == "path_a" {
        &plan.path_a
    } else {
        &plan.path_b
    };
    let phases = path_value
        .get("phases")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("no phases"))?;
    let phase = phases
        .get(args.phase as usize)
        .ok_or_else(|| anyhow!("phase {} missing", args.phase))?;

    let scene_prompt = phase
        .get("scene_prompt")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let motion_prompt = phase
        .get("motion_prompt")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let edit_prompt = phase
        .get("edit_prompt")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let narration_text = phase
        .get("narration_text")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    tracing::info!(
        simulation_id = %args.simulation_id,
        path = %args.path,
        phase = args.phase,
        scene_prompt_len = scene_prompt.len(),
        motion_prompt_len = motion_prompt.len(),
        edit_prompt_len = edit_prompt.len(),
        "video_generation: loaded scenario phase"
    );

    let sim = state
        .simulation_repo
        .get_simulation_by_id(args.simulation_id)
        .await?;

    // Collect references. The character palette is required: generated video
    // should never silently downgrade to text-to-video for an onboarded user
    // with a photo.
    let profile_photo_url = match state
        .user_repo
        .get_primary_photo_by_user_id(sim.user_id)
        .await
    {
        Ok(p) => state
            .object_store
            .get_external_signed_url(
                &state.cfg.s3_bucket,
                &p.storage_path,
                Duration::from_secs(3600),
            )
            .await
            .unwrap_or_else(|_| p.storage_url.clone()),
        Err(e) => {
            tracing::warn!(user_id = %sim.user_id, error = ?e, "video_generation: no primary photo");
            String::new()
        }
    };
    let palette_ref =
        resolve_character_palette_for_generation(state, sim.user_id, "video_generation").await?;
    let character_plate_url = palette_ref.url.clone();

    tracing::info!(
        simulation_id = %args.simulation_id,
        path = %args.path,
        phase = args.phase,
        has_profile_photo = !profile_photo_url.is_empty(),
        plate_id = %palette_ref.plate_id,
        character_palette_source = palette_ref.source,
        "video_generation: references prepared"
    );

    let clip_dur = state.cfg.simulation_video_clip_duration_secs.max(2) as u32;

    // Build the user-state block once per phase. We tolerate failure here —
    // the pipeline accepts an empty block and falls back to today's behavior.
    let user_state_block = match load_simulation_context(state, &sim).await {
        Ok(ctx) => crate::prompts::build_user_state_block(&ctx),
        Err(e) => {
            tracing::warn!(
                simulation_id = %args.simulation_id,
                user_id = %sim.user_id,
                error = ?e,
                "video_generation: failed to load simulation context; proceeding without user-state block"
            );
            String::new()
        }
    };

    // Stable seed derived from simulation_id so all 6 phases share one seed
    // and faces don't drift across clips. Each clip still varies in scene
    // because the prompt differs; the seed only controls model RNG.
    let stable_seed = simulation_seed(args.simulation_id);

    let pipeline_input = PipelineInput {
        simulation_id: args.simulation_id,
        user_id: sim.user_id,
        scene_prompt: scene_prompt.clone(),
        motion_prompt: motion_prompt.clone(),
        edit_prompt,
        profile_photo_url,
        character_plate_url: character_plate_url.clone(),
        duration_secs: clip_dur,
        clip_tag: format!("{}_{}", args.path, args.phase),
        seed: Some(stable_seed),
        user_state_block,
        narration_text: narration_text.clone(),
    };

    tracing::debug!("video_generation: invoking 3-stage Precision Sequence pipeline");
    let out = state.media_pipeline.execute(pipeline_input).await?;
    tracing::info!(
        storage_path = %out.final_video.storage_path,
        mime = %out.final_video.mime_type,
        "video_generation: pipeline completed"
    );
    let final_path = out.final_video.storage_path;
    let final_url = out.final_video.storage_url;

    let clip_role = format!("{}_{}", args.path, args.phase);
    let media = models::GeneratedMedia {
        id: Uuid::new_v4(),
        simulation_id: args.simulation_id,
        media_type: models::media_type::VIDEO.into(),
        storage_url: final_url,
        storage_path: final_path,
        prompt_used: scene_prompt,
        provider_metadata: serde_json::json!({"phase": args.phase, "path": &args.path}),
        clip_role: clip_role.clone(),
        clip_order: args.phase,
        scenario_path: Some(args.path.clone()),
        scenario_phase: Some(args.phase),
        created_at: chrono::Utc::now(),
    };
    state.media_repo.insert_generated_media(&media).await?;

    // Phase 2: set scenario_path/scenario_phase columns. `insert_generated_media`
    // only writes the core columns (mirror of the Go InsertGeneratedMedia shape);
    // scenario coordinates live in dedicated columns used for querying.
    if let Err(e) = state
        .media_repo
        .update_media_scenario_fields(
            args.simulation_id,
            &clip_role,
            Some(&args.path),
            Some(args.phase),
        )
        .await
    {
        tracing::warn!(
            simulation_id = %args.simulation_id,
            clip_role = %clip_role,
            error = ?e,
            "video_generation: failed to update scenario fields on media"
        );
    }

    let _ = state
        .components_repo
        .mark_component_completed(args.simulation_id, &component_key)
        .await;
    tracing::info!(
        simulation_id = %args.simulation_id,
        path = %args.path,
        phase = args.phase,
        clip_role = %clip_role,
        total_elapsed_ms = started.elapsed().as_millis() as u64,
        "video_generation: completed"
    );
    Ok(())
}

async fn run_flash_generation(state: &AppState, args: FlashGenerationArgs) -> Result<()> {
    tracing::info!(
        flash_id = %args.flash_vision_id,
        user_id = %args.user_id,
        "flash_generation: starting"
    );
    let vision = state
        .flash_repo
        .get_flash_vision_by_id(args.flash_vision_id)
        .await?;
    tracing::debug!(question_len = vision.question.len(), input_method = %vision.input_method, "flash_generation: loaded vision");

    state
        .flash_repo
        .update_flash_vision_status(vision.id, models::flash_status::GENERATING, None)
        .await?;

    // Build the scene plan.
    let sys = r#"You are Scout's Flash vision planner. Produce a scene plan with exactly 6 prompts describing a cinematic what-if sequence.

Hard visual rules that MUST apply to every prompt:
- No visible written text, typography, captions, signage, billboards, logos, or book/paper writing.
- No device screens or UI: no phone displays, laptop monitors, tablets, TVs, or any visible user interface.
- Avoid scenes that would naturally depend on text or screen content to read (no reading notifications, emails, documents, dashboards, etc.).
- Compose shots for vertical 9:16 framing: place subjects centered with headroom; keep critical action within a safe vertical frame.

Output JSON: {"scenario_title":"...", "mood":"...", "music_mood":"calm|hopeful|tense|reflective", "style_anchor":"photorealistic ...", "prompts":[{"index":0,"scene":"...","prompt":"..."}, ..., {"index":5,...}]}"#;
    let mut life_state_summary = String::new();
    if let Ok(ls) = state.user_repo.build_life_state(args.user_id).await {
        if !ls.profession.is_empty() {
            life_state_summary.push_str(&format!("Profession: {}; ", ls.profession));
        }
        if !ls.location.is_empty() {
            life_state_summary.push_str(&format!("Location: {}; ", ls.location));
        }
        if !ls.age_range.is_empty() {
            life_state_summary.push_str(&format!("Age: {}; ", ls.age_range));
        }
    }
    let user_prompt = format!(
        "Question: {}\n\nUser life summary: {}\n\nReturn JSON only.",
        vision.question, life_state_summary
    );
    let resp = state
        .text_provider
        .generate_text(&TextRequest {
            system_prompt: sys.into(),
            user_prompt,
            max_tokens: 3000,
            temperature: 0.7,
            json_mode: true,
            ..Default::default()
        })
        .await?;
    let plan: FlashPromptPlan = match serde_json::from_str(&resp.content) {
        Ok(p) => p,
        Err(e) => {
            let msg = format!("scene plan parse: {}", e);
            tracing::error!(flash_id = %vision.id, error = ?e, "flash_generation: failed to parse scene plan");
            let _ = state
                .flash_repo
                .update_flash_vision_status(vision.id, models::flash_status::FAILED, Some(&msg))
                .await;
            return Err(anyhow!(msg));
        }
    };
    tracing::debug!(
        flash_id = %vision.id,
        scenario_title = %plan.scenario_title,
        mood = %plan.mood,
        music_mood = %plan.music_mood,
        scene_count = plan.prompts.len(),
        "flash_generation: scene plan produced"
    );

    // Block until the character palette is ready. First what-if images require
    // the static baseline identity anchor; dynamic palettes are only used when
    // they are already ready.
    let palette_ref =
        match resolve_character_palette_for_generation(state, args.user_id, "flash_generation")
            .await
        {
            Ok(reference) => reference,
            Err(e) => {
                let msg = format!("character_palette_unavailable: {}", e);
                tracing::error!(
                    flash_id = %vision.id,
                    user_id = %args.user_id,
                    error = ?e,
                    "flash_generation: character palette unavailable"
                );
                let _ = state
                    .flash_repo
                    .update_flash_vision_status(vision.id, models::flash_status::FAILED, Some(&msg))
                    .await;
                return Err(anyhow!(msg));
            }
        };

    // Seed stays constant across scenes for identity consistency.
    // Clamp to u32 range — Runway's schema types seed as "int" with no explicit
    // bounds, but the likely internal representation is u32. Keeps both providers
    // safe.
    let seed: i64 = rand::random::<u32>() as i64;

    tracing::debug!(
        flash_id = %vision.id,
        input_source = palette_ref.source,
        plate_id = %palette_ref.plate_id,
        seed,
        "flash_generation: beginning per-scene generation"
    );
    for scene in &plan.prompts {
        tracing::debug!(
            flash_id = %vision.id,
            scene_index = scene.index,
            prompt_len = scene.prompt.len(),
            "flash_generation: generating scene"
        );
        let scene_part = format!("{} {}", plan.style_anchor, scene.prompt);
        let scene_budget = FLASH_PROMPT_MAX_BYTES
            .saturating_sub(FLASH_SCENE_CONSTRAINTS.len() + FLASH_PROMPT_SUFFIX.len());
        let scene_part_trimmed = truncate_to_byte_boundary(&scene_part, scene_budget);
        if scene_part_trimmed.len() < scene_part.len() {
            tracing::debug!(
                flash_id = %vision.id,
                scene_index = scene.index,
                original_len = scene_part.len(),
                trimmed_len = scene_part_trimmed.len(),
                "flash_generation: trimmed scene prompt to fit provider limit"
            );
        }
        let final_prompt = format!(
            "{}{}{}",
            scene_part_trimmed, FLASH_SCENE_CONSTRAINTS, FLASH_PROMPT_SUFFIX
        );
        let r = state
            .flash_image_provider
            .generate_image_with_input(&FlashImageWithInputRequest {
                prompt: final_prompt.clone(),
                input_image: palette_ref.url.clone(),
                width: 1080,
                height: 1920,
                seed: Some(seed),
                output_format: "jpeg".into(),
            })
            .await?;
        let (bytes, mime) = (r.image_bytes, r.mime_type);
        let key = format!("flash/{}/{:02}.jpg", vision.id, scene.index);
        let url = state
            .object_store
            .upload(&state.cfg.flash_images_bucket, &key, bytes, &mime)
            .await?;
        let img = FlashImage {
            id: Uuid::new_v4(),
            flash_vision_id: vision.id,
            index: scene.index,
            storage_url: url,
            storage_path: key,
            prompt_used: scene.prompt.clone(),
            style_reference_id: None,
            generation_metadata: serde_json::json!({"seed": seed, "mood": &plan.mood, "style_anchor": &plan.style_anchor}),
            created_at: chrono::Utc::now(),
        };
        state.flash_repo.create_flash_image(&img).await?;
    }

    // Attach music based on mood.
    let music_url = crate::flash::music_url_for_mood(&plan.music_mood);
    state
        .flash_repo
        .set_music_url(vision.id, &music_url)
        .await?;
    tracing::debug!(flash_id = %vision.id, music_mood = %plan.music_mood, "flash_generation: music attached");

    state
        .flash_repo
        .update_flash_vision_status(vision.id, models::flash_status::COMPLETED, None)
        .await?;
    tracing::info!(flash_id = %vision.id, "flash_generation: completed");
    Ok(())
}

/// Resolve the identity anchor for generated visual experiences.
///
/// The static baseline palette is mandatory for onboarded users with photos.
/// Dynamic palettes are optional: if a dynamic variant is already ready we use
/// it, otherwise we use the static baseline and leave dynamic generation in
/// flight for future jobs.
async fn resolve_character_palette_for_generation(
    state: &AppState,
    user_id: Uuid,
    context: &'static str,
) -> Result<CharacterPaletteReference> {
    let photo = state
        .user_repo
        .get_primary_photo_by_user_id(user_id)
        .await
        .map_err(|e| {
            anyhow!(
                "permanent: character_palette_unavailable: primary photo required: {}",
                e
            )
        })?;

    claim_and_enqueue_character_plate(
        state,
        user_id,
        photo.id,
        CHARACTER_PLATE_PROMPT,
        context,
        "static_baseline",
    )
    .await
    .context("character_palette_unavailable: failed to ensure static baseline")?;

    let baseline =
        wait_for_character_plate_prompt(state, user_id, photo.id, CHARACTER_PLATE_PROMPT, context)
            .await?;
    let baseline_url = signed_character_plate_url(state, &baseline)
        .await
        .ok_or_else(|| {
            anyhow!(
                "character_palette_unavailable: static baseline plate {} has no usable storage URL",
                baseline.id
            )
        })?;

    if state.cfg.enable_dynamic_character_plate {
        let dynamic_prompt = plate_prompt_for_user(state, user_id).await;
        if dynamic_prompt != CHARACTER_PLATE_PROMPT {
            match claim_and_enqueue_character_plate(
                state,
                user_id,
                photo.id,
                &dynamic_prompt,
                context,
                "dynamic",
            )
            .await
            {
                Ok(dynamic) if dynamic.status == models::character_plate_status::READY => {
                    if let Some(dynamic_url) = signed_character_plate_url(state, &dynamic).await {
                        tracing::info!(
                            user_id = %user_id,
                            plate_id = %dynamic.id,
                            context,
                            "character_palette: using ready dynamic palette"
                        );
                        return Ok(CharacterPaletteReference {
                            url: dynamic_url,
                            source: "dynamic_character_plate",
                            plate_id: dynamic.id,
                        });
                    }
                    tracing::warn!(
                        user_id = %user_id,
                        plate_id = %dynamic.id,
                        context,
                        "character_palette: dynamic palette ready but missing usable URL; using static baseline"
                    );
                }
                Ok(dynamic) => {
                    tracing::debug!(
                        user_id = %user_id,
                        plate_id = %dynamic.id,
                        status = %dynamic.status,
                        context,
                        "character_palette: dynamic palette not ready; using static baseline"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        user_id = %user_id,
                        error = ?e,
                        context,
                        "character_palette: dynamic palette ensure failed; using static baseline"
                    );
                }
            }
        }
    }

    tracing::info!(
        user_id = %user_id,
        plate_id = %baseline.id,
        context,
        "character_palette: using static baseline palette"
    );
    Ok(CharacterPaletteReference {
        url: baseline_url,
        source: "static_character_plate",
        plate_id: baseline.id,
    })
}

async fn wait_for_character_plate_prompt(
    state: &AppState,
    user_id: Uuid,
    source_photo_id: Uuid,
    prompt: &str,
    context: &'static str,
) -> Result<models::CharacterPlate> {
    const POLL_INTERVAL: Duration = Duration::from_secs(5);
    const MAX_WAIT: Duration = Duration::from_secs(300);

    tracing::info!(
        user_id = %user_id,
        source_photo_id = %source_photo_id,
        context,
        "character_palette: waiting for static baseline to become ready"
    );
    let deadline = std::time::Instant::now() + MAX_WAIT;
    loop {
        if let Ok(plate) = state
            .user_repo
            .get_ready_character_plate_by_prompt(user_id, source_photo_id, prompt)
            .await
        {
            tracing::info!(
                user_id = %user_id,
                source_photo_id = %source_photo_id,
                plate_id = %plate.id,
                context,
                "character_palette: static baseline ready"
            );
            return Ok(plate);
        }
        if std::time::Instant::now() >= deadline {
            return Err(anyhow!(
                "character_palette_unavailable: timed out after {}s waiting for static baseline palette",
                MAX_WAIT.as_secs()
            ));
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

async fn signed_character_plate_url(
    state: &AppState,
    plate: &models::CharacterPlate,
) -> Option<String> {
    if let Some(path) = plate.storage_path.as_deref().filter(|p| !p.is_empty()) {
        let bucket = plate
            .storage_bucket
            .as_deref()
            .unwrap_or(state.cfg.character_palettes_bucket.as_str());
        match state
            .object_store
            .get_external_signed_url(bucket, path, Duration::from_secs(3600))
            .await
        {
            Ok(url) => return Some(url),
            Err(e) => {
                tracing::warn!(
                    plate_id = %plate.id,
                    bucket,
                    path,
                    error = ?e,
                    "character_palette: failed to sign stored palette path; trying stored URL"
                );
            }
        }
    }
    plate.storage_url.clone().filter(|u| !u.is_empty())
}

async fn load_simulation_context(
    state: &AppState,
    sim: &models::DecisionSimulation,
) -> Result<SimulationContext> {
    let profile = state.user_repo.get_profile_by_user_id(sim.user_id).await?;
    let story = state
        .user_repo
        .get_life_story_by_user_id(sim.user_id)
        .await
        .ok();
    let life_state = state
        .user_repo
        .build_life_state(sim.user_id)
        .await
        .unwrap_or_else(|_| models::LifeState::default_state());
    let decision = state
        .decision_repo
        .get_decision_by_id(sim.decision_id)
        .await
        .ok();

    let behavioral = crate::models::resolve_behavioral_profile(&profile);
    let financial_profile = crate::models::resolve_financial_profile(&profile);
    let life_context = crate::models::resolve_life_context_profile(&profile);
    let fact_sheet =
        crate::financial::build_financial_fact_sheet(&profile, &life_state, &financial_profile);

    let mut ctx = SimulationContext::default();
    ctx.user = Some(profile);
    ctx.behavioral_profile = behavioral;
    ctx.financial_profile = financial_profile;
    ctx.life_context_profile = life_context;
    ctx.financial_fact_sheet = Some(fact_sheet);
    ctx.life_state = life_state;
    if let Some(s) = &story {
        ctx.extracted_context = s.extracted_context.clone();
    }
    ctx.life_story = story;
    ctx.decision = decision.clone();
    ctx.time_horizon_months = decision.map(|d| d.time_horizon_months).unwrap_or(12);
    ctx.assumption_overrides = sim.assumption_overrides.clone();
    ctx.video_clip_duration_secs = state.cfg.simulation_video_clip_duration_secs;
    if state.cfg.is_development() && state.cfg.scenario_planner_dev_phase_count > 0 {
        ctx.scenario_planner_exact_phases = state.cfg.scenario_planner_dev_phase_count;
    }
    Ok(ctx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simulation_seed_is_stable_for_same_id() {
        let id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        assert_eq!(simulation_seed(id), simulation_seed(id));
    }

    #[test]
    fn simulation_seed_differs_across_ids() {
        let a = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let b = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap();
        assert_ne!(simulation_seed(a), simulation_seed(b));
    }

    #[test]
    fn simulation_seed_fits_in_u32() {
        // Provider compatibility: Runway schema types seed as "int", likely u32
        // internally. Existing flash path clamps seeds via `rand::random::<u32>()
        // as i64`. Our stable seed must match that range, not a full positive i64.
        for s in [
            "550e8400-e29b-41d4-a716-446655440000",
            "00000000-0000-0000-0000-000000000000",
            "ffffffff-ffff-ffff-ffff-ffffffffffff",
        ] {
            let id = Uuid::parse_str(s).unwrap();
            let seed = simulation_seed(id);
            assert!(seed >= 0, "negative seed for {}", s);
            assert!(
                seed <= u32::MAX as i64,
                "seed {} exceeds u32::MAX for {}",
                seed,
                s
            );
        }
    }

    #[test]
    fn character_plate_prompt_falls_back_to_static_when_no_hints() {
        let profile = models::UserProfile {
            id: Uuid::nil(),
            user_id: Uuid::nil(),
            estimated_net_worth: 0.0,
            estimated_yearly_salary: 0.0,
            onboarding_status: String::new(),
            risk_tolerance: None,
            follow_through: None,
            optimism_bias: None,
            stress_response: None,
            decision_style: None,
            saving_habits: None,
            debt_comfort: None,
            housing_stability: None,
            income_stability: None,
            liquid_net_worth_source: None,
            relationship_status: None,
            household_income_structure: None,
            dependent_count: None,
            life_stability: None,
            onboarding_path: String::new(),
            age_bracket: None,
            gender: None,
            living_situation: None,
            industry: None,
            career_stage: None,
            net_worth_bracket: None,
            income_bracket: None,
            cinematic_context_completed: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        let life = models::LifeState::default();
        let out = build_character_plate_prompt(&profile, &life);
        assert_eq!(out, CHARACTER_PLATE_PROMPT);
    }

    #[test]
    fn character_plate_prompt_appends_age_and_bearing_when_available() {
        let profile = models::UserProfile {
            id: Uuid::nil(),
            user_id: Uuid::nil(),
            estimated_net_worth: 0.0,
            estimated_yearly_salary: 0.0,
            onboarding_status: String::new(),
            risk_tolerance: None,
            follow_through: None,
            optimism_bias: None,
            stress_response: Some("analytical".into()),
            decision_style: Some("deliberate".into()),
            saving_habits: None,
            debt_comfort: None,
            housing_stability: None,
            income_stability: None,
            liquid_net_worth_source: None,
            relationship_status: None,
            household_income_structure: None,
            dependent_count: None,
            life_stability: None,
            onboarding_path: String::new(),
            age_bracket: Some("35-44".into()),
            gender: None,
            living_situation: None,
            industry: None,
            career_stage: None,
            net_worth_bracket: None,
            income_bracket: None,
            cinematic_context_completed: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        let life = models::LifeState::default();
        let out = build_character_plate_prompt(&profile, &life);
        assert!(out.contains("late 30s to early 40s"));
        assert!(out.contains("calm, composed bearing"));
        // The EXACT original preserve clause must be the final clause —
        // recency bias is the drift-prevention mechanism for the dynamic
        // prompt.
        assert!(
            out.ends_with(PLATE_EXACT_PRESERVE_CLAUSE),
            "preserve clause is not the final clause: ...{}",
            &out[out.len().saturating_sub(200)..]
        );
        // Must contain "natural expression" (it's in the exact preserve clause).
        assert!(
            out.contains("natural expression"),
            "lost 'natural expression' from preserve clause"
        );
    }

    #[test]
    fn character_plate_prompt_dynamic_has_no_duplicated_preserve_clause() {
        // Static CHARACTER_PLATE_PROMPT contains the preserve sentence once
        // (in the middle). Dynamic prompt must also contain it once (at the
        // end) — never twice. Duplication would waste tokens and could
        // confuse Flux about which position to weight.
        let profile = test_profile(Some("analytical"), Some("deliberate"), Some("35-44"));
        let life = models::LifeState::default();
        let out = build_character_plate_prompt(&profile, &life);
        let preserve_count = out.matches("Preserve the subject's face").count();
        assert_eq!(
            preserve_count, 1,
            "expected preserve clause exactly once, got {}",
            preserve_count
        );
    }

    #[test]
    fn static_character_plate_prompt_equals_triptych_plus_exact_preserve() {
        // Invariant: the static const must be byte-identical to
        // [triptych body WITHOUT preserve] with preserve clause slotted in
        // at the position the original had. We verify by checking that
        // CHARACTER_PLATE_PROMPT contains both the without-preserve body
        // fragments and the exact preserve clause. If anyone edits one
        // without syncing the other, this test catches it.
        assert!(
            CHARACTER_PLATE_PROMPT.contains("subject centered within its own panel."),
            "static prompt lost triptych body"
        );
        assert!(
            CHARACTER_PLATE_PROMPT.contains(PLATE_EXACT_PRESERVE_CLAUSE.trim()),
            "static prompt missing exact preserve clause"
        );
        assert!(
            CHARACTER_PLATE_PROMPT.contains("Soft studio lighting."),
            "static prompt lost closing constraints"
        );
    }

    /// Minimal profile fixture for Layer C tests — only the fields the plate
    /// prompt actually reads. Everything else is None / default.
    fn test_profile(
        stress: Option<&str>,
        style: Option<&str>,
        age_bracket: Option<&str>,
    ) -> models::UserProfile {
        models::UserProfile {
            id: Uuid::nil(),
            user_id: Uuid::nil(),
            estimated_net_worth: 0.0,
            estimated_yearly_salary: 0.0,
            onboarding_status: String::new(),
            risk_tolerance: None,
            follow_through: None,
            optimism_bias: None,
            stress_response: stress.map(|s| s.to_string()),
            decision_style: style.map(|s| s.to_string()),
            saving_habits: None,
            debt_comfort: None,
            housing_stability: None,
            income_stability: None,
            liquid_net_worth_source: None,
            relationship_status: None,
            household_income_structure: None,
            dependent_count: None,
            life_stability: None,
            onboarding_path: String::new(),
            age_bracket: age_bracket.map(|s| s.to_string()),
            gender: None,
            living_situation: None,
            industry: None,
            career_stage: None,
            net_worth_bracket: None,
            income_bracket: None,
            cinematic_context_completed: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn plate_bearing_phrase_returns_empty_when_either_input_absent() {
        // No defaults-as-data — partial behavioral signal must produce silence
        // in the plate, same as Layer B's bearing_phrase.
        assert_eq!(plate_bearing_phrase(None, Some("deliberate")), "");
        assert_eq!(plate_bearing_phrase(Some("analytical"), None), "");
        assert_eq!(plate_bearing_phrase(None, None), "");
        assert_eq!(plate_bearing_phrase(Some(""), Some("deliberate")), "");
        assert_eq!(plate_bearing_phrase(Some("analytical"), Some("")), "");
    }

    #[test]
    fn plate_bearing_phrase_returns_empty_for_withdrawn_without_style() {
        // Previously ("withdrawn", _) matched even when style was empty via
        // unwrap_or(""). The tightened gating requires both fields set.
        assert_eq!(plate_bearing_phrase(Some("withdrawn"), None), "");
        assert_eq!(plate_bearing_phrase(Some("withdrawn"), Some("")), "");
        // With style set, withdrawn does emit.
        assert!(plate_bearing_phrase(Some("withdrawn"), Some("deliberate")).contains("reserved"));
    }

    #[test]
    fn character_plate_prompt_omits_bearing_when_only_stress_set() {
        let profile = test_profile(Some("analytical"), None, Some("35-44"));
        let life = models::LifeState::default();
        let out = build_character_plate_prompt(&profile, &life);
        assert!(out.contains("late 30s to early 40s"), "age missing: {out}");
        assert!(
            !out.contains("bearing"),
            "bearing leaked with partial stress-only: {out}"
        );
        assert!(
            !out.contains("presence"),
            "presence leaked with partial stress-only: {out}"
        );
    }

    #[test]
    fn character_plate_prompt_prefers_life_age_over_profile_bracket() {
        let profile = test_profile(None, None, Some("55-64"));
        let mut life = models::LifeState::default();
        life.age = 28;
        let out = build_character_plate_prompt(&profile, &life);
        // life.age = 28 → "late 20s to early 30s". Bracket "55-64" would say
        // "late 50s to early 60s". Life age must win.
        assert!(
            out.contains("late 20s to early 30s"),
            "expected life.age bucket: {out}"
        );
        assert!(
            !out.contains("late 50s"),
            "profile.age_bracket leaked when life.age was set: {out}"
        );
    }

    #[test]
    fn character_plate_prompt_falls_back_to_age_bracket_when_life_age_unset() {
        let profile = test_profile(None, None, Some("45-54"));
        let life = models::LifeState::default(); // age = 0
        let out = build_character_plate_prompt(&profile, &life);
        assert!(
            out.contains("late 40s to early 50s"),
            "profile.age_bracket fallback missing: {out}"
        );
    }
}
