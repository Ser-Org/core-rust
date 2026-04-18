use crate::app_state::AppState;
use crate::jobs::*;
use crate::media::{MediaPipeline, PipelineInput};
use crate::models::{self, FlashImage, FlashPromptPlan};
use crate::prompts::{self, SimulationContext};
use crate::providers::{FlashImageRequest, FlashImageWithInputRequest, TextRequest};
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

/// Trim `s` to at most `max_bytes`, backing up to the nearest UTF-8 char
/// boundary. Never panics on multi-byte chars.
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
            run_video_generation(state, args).await
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
    let story = state.user_repo.get_life_story_by_user_id(args.user_id).await?;
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
        state.user_repo.update_extracted_context(args.story_id, &parsed).await?;
        tracing::info!(
            story_id = %args.story_id,
            field_count = parsed.as_object().map(|o| o.len()).unwrap_or(0),
            "life_state_extraction: persisted extracted_context"
        );
    } else {
        tracing::warn!("life_state_extraction: AI returned unparseable JSON, leaving context unchanged");
    }
    Ok(())
}

async fn run_scenario_planner(state: &AppState, args: ScenarioPlannerArgs) -> Result<()> {
    tracing::info!(simulation_id = %args.simulation_id, "scenario_planner: starting");
    let sim = state.simulation_repo.get_simulation_by_id(args.simulation_id).await?;
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
    let parsed: JsonValue = serde_json::from_str(&resp.content).context("parse scenario planner")?;
    let path_a = parsed.get("path_a").cloned().unwrap_or(JsonValue::Null);
    let path_b = parsed.get("path_b").cloned().unwrap_or(JsonValue::Null);
    let shared = parsed.get("shared_context").cloned().unwrap_or(JsonValue::Null);

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
            &AssumptionExtractionArgs { simulation_id: args.simulation_id },
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
            components.push(video_component(
                args.simulation_id,
                "path_a",
                i as i32,
            ));
        }
        for (i, _) in phases_b.iter().enumerate() {
            components.push(video_component(
                args.simulation_id,
                "path_b",
                i as i32,
            ));
        }
        state
            .components_repo
            .upsert_simulation_components(&components)
            .await?;

        // Enqueue character plate (single-flight per user/photo) and video generation jobs.
        match state.user_repo.get_primary_photo_by_user_id(sim.user_id).await {
            Ok(photo) => {
                let (plate, claimed) = state
                    .user_repo
                    .claim_character_plate_generation(sim.user_id, photo.id, CHARACTER_PLATE_PROMPT)
                    .await?;
                if claimed {
                    tracing::info!(
                        plate_id = %plate.id,
                        source_photo_id = %photo.id,
                        user_id = %sim.user_id,
                        "scenario_planner: claimed character plate generation, enqueuing job"
                    );
                    state
                        .job_client
                        .insert(
                            KIND_CHARACTER_PLATE,
                            &CharacterPlateArgs {
                                user_id: sim.user_id,
                                source_photo_id: photo.id,
                                plate_id: plate.id,
                            },
                        )
                        .await?;
                } else {
                    tracing::debug!(
                        plate_id = %plate.id,
                        status = %plate.status,
                        "scenario_planner: character plate already in flight or ready, reusing"
                    );
                }
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

    let sim = state.simulation_repo.get_simulation_by_id(args.simulation_id).await?;
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
                kind: a.get("kind").and_then(|v| v.as_str()).unwrap_or("").to_string(),
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
    state.simulation_repo.bulk_insert_assumptions(&assumptions).await?;
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
    let photo = state
        .user_repo
        .get_primary_photo_by_user_id(args.user_id)
        .await
        .map_err(|e| anyhow!("get primary photo: {}", e))?;

    let photo_url = state
        .object_store
        .get_external_signed_url(
            &state.cfg.s3_bucket,
            &photo.storage_path,
            Duration::from_secs(3600),
        )
        .await
        .unwrap_or_else(|_| photo.storage_url.clone());

    let resp = match state
        .flash_image_provider
        .generate_image_with_input(&FlashImageWithInputRequest {
            prompt: CHARACTER_PLATE_PROMPT.into(),
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

    let path = format!("character-plates/{}/{}.png", args.user_id, photo.id);
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
    let path_value = if args.path == "path_a" { &plan.path_a } else { &plan.path_b };
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

    tracing::info!(
        simulation_id = %args.simulation_id,
        path = %args.path,
        phase = args.phase,
        scene_prompt_len = scene_prompt.len(),
        motion_prompt_len = motion_prompt.len(),
        edit_prompt_len = edit_prompt.len(),
        "video_generation: loaded scenario phase"
    );

    let sim = state.simulation_repo.get_simulation_by_id(args.simulation_id).await?;

    // Collect references: profile photo + character plate (best-effort).
    let profile_photo_url = match state.user_repo.get_primary_photo_by_user_id(sim.user_id).await {
        Ok(p) => state
            .object_store
            .get_external_signed_url(&state.cfg.s3_bucket, &p.storage_path, Duration::from_secs(3600))
            .await
            .unwrap_or_else(|_| p.storage_url.clone()),
        Err(e) => {
            tracing::warn!(user_id = %sim.user_id, error = ?e, "video_generation: no primary photo");
            String::new()
        }
    };
    let character_plate_url = match state.user_repo.get_ready_character_plate_by_user_id(sim.user_id).await
    {
        Ok(plate) => match (plate.storage_bucket.as_deref(), plate.storage_path.as_deref()) {
            (Some(bucket), Some(path)) => state
                .object_store
                .get_external_signed_url(bucket, path, Duration::from_secs(3600))
                .await
                .unwrap_or_else(|_| plate.storage_url.clone().unwrap_or_default()),
            _ => plate.storage_url.clone().unwrap_or_default(),
        },
        Err(e) => {
            tracing::warn!(user_id = %sim.user_id, error = ?e, "video_generation: character plate not ready");
            String::new()
        }
    };

    tracing::info!(
        simulation_id = %args.simulation_id,
        path = %args.path,
        phase = args.phase,
        has_profile_photo = !profile_photo_url.is_empty(),
        has_character_plate = !character_plate_url.is_empty(),
        "video_generation: references prepared"
    );

    let clip_dur = state.cfg.simulation_video_clip_duration_secs.max(2) as u32;
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
        seed: None,
    };

    // If no character plate, fall back to a single-call gen4.5 text-to-video.
    let (final_path, final_url, mime) = if character_plate_url.is_empty() {
        tracing::warn!(
            simulation_id = %args.simulation_id,
            "video_generation: no character plate available, falling back to single-stage text-to-video"
        );
        let resp = state
            .video_provider
            .generate_video(&crate::providers::VideoRequest {
                prompt: format!("{}. {}", scene_prompt, motion_prompt),
                duration_secs: clip_dur,
                model: "".into(),
                aspect_ratio: crate::media::DEFAULT_PIPELINE_VIDEO_ASPECT_RATIO.into(),
                input_image_url: String::new(),
                input_video_url: String::new(),
                reference_images: vec![],
                seed: None,
            })
            .await?;
        let clip_key = format!(
            "simulations/{}/{}/{}.mp4",
            args.simulation_id, args.path, args.phase
        );
        let url = state
            .object_store
            .upload(&state.cfg.s3_bucket, &clip_key, resp.video_data, &resp.mime_type)
            .await?;
        tracing::debug!(clip_key = %clip_key, "video_generation: fallback video uploaded");
        (clip_key, url, resp.mime_type)
    } else {
        tracing::debug!("video_generation: invoking 3-stage Precision Sequence pipeline");
        let out = state.media_pipeline.execute(pipeline_input).await?;
        tracing::info!(
            storage_path = %out.final_video.storage_path,
            mime = %out.final_video.mime_type,
            "video_generation: pipeline completed"
        );
        (out.final_video.storage_path, out.final_video.storage_url, out.final_video.mime_type)
    };

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
    let vision = state.flash_repo.get_flash_vision_by_id(args.flash_vision_id).await?;
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

    // Block until the character palette is ready — flash images require it as the
    // identity reference. Self-heals: if no generation is in flight yet (e.g. old
    // user who onboarded before this path was added), claim and enqueue one here.
    let mut input_image_url: Option<String> = None;
    let mut input_source: &'static str = "none";
    let plate = wait_for_character_plate(state, args.user_id).await;
    if let Some(plate) = plate {
        if let Some(path) = plate.storage_path.as_deref() {
            let bucket = plate
                .storage_bucket
                .as_deref()
                .unwrap_or(&state.cfg.character_palettes_bucket);
            match state
                .object_store
                .get_external_signed_url(bucket, path, Duration::from_secs(3600))
                .await
            {
                Ok(url) => {
                    input_image_url = Some(url);
                    input_source = "character_plate";
                }
                Err(_) => {
                    if let Some(url) = plate.storage_url.clone().filter(|u| !u.is_empty()) {
                        input_image_url = Some(url);
                        input_source = "character_plate";
                    }
                }
            }
        }
    }
    if input_image_url.is_none() {
        tracing::warn!(
            flash_id = %vision.id,
            user_id = %args.user_id,
            "flash_generation: character palette unavailable, falling back to raw user photo"
        );
        if let Ok(p) = state.user_repo.get_flux_photo_by_user_id(args.user_id).await {
            let bucket = &state.cfg.s3_bucket;
            let path = p.flux_storage_path.clone().unwrap_or(p.storage_path.clone());
            match state
                .object_store
                .get_external_signed_url(bucket, &path, Duration::from_secs(3600))
                .await
            {
                Ok(url) => {
                    input_image_url = Some(url);
                    input_source = "flux_photo";
                }
                Err(_) => {
                    if !p.storage_url.is_empty() {
                        input_image_url = Some(p.storage_url.clone());
                        input_source = "flux_photo";
                    }
                }
            }
        }
    }

    // Seed stays constant across scenes for identity consistency.
    // Clamp to u32 range — Runway's schema types seed as "int" with no explicit
    // bounds, but the likely internal representation is u32. Keeps both providers
    // safe.
    let seed: i64 = rand::random::<u32>() as i64;

    let use_input_image = input_image_url.is_some();
    tracing::debug!(
        flash_id = %vision.id,
        use_input_image,
        input_source,
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
        let (bytes, mime) = if let Some(img_url) = &input_image_url {
            let r = state
                .flash_image_provider
                .generate_image_with_input(&FlashImageWithInputRequest {
                    prompt: final_prompt.clone(),
                    input_image: img_url.clone(),
                    width: 1080,
                    height: 1920,
                    seed: Some(seed),
                    output_format: "jpeg".into(),
                })
                .await?;
            (r.image_bytes, r.mime_type)
        } else {
            let r = state
                .flash_image_provider
                .generate_image(&FlashImageRequest {
                    prompt: final_prompt.clone(),
                    width: 1080,
                    height: 1920,
                    seed: Some(seed),
                    output_format: "jpeg".into(),
                })
                .await?;
            (r.image_bytes, r.mime_type)
        };
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
    state.flash_repo.set_music_url(vision.id, &music_url).await?;
    tracing::debug!(flash_id = %vision.id, music_mood = %plan.music_mood, "flash_generation: music attached");

    state
        .flash_repo
        .update_flash_vision_status(vision.id, models::flash_status::COMPLETED, None)
        .await?;
    tracing::info!(flash_id = %vision.id, "flash_generation: completed");
    Ok(())
}

/// Block until the character palette for this user is `ready`, self-healing if
/// no generation is in flight. Returns `None` if we can't generate one (no
/// primary photo) or the palette fails / times out — the caller is expected to
/// fall back to the raw photo.
async fn wait_for_character_plate(
    state: &AppState,
    user_id: Uuid,
) -> Option<crate::models::CharacterPlate> {
    const POLL_INTERVAL: Duration = Duration::from_secs(5);
    const MAX_WAIT: Duration = Duration::from_secs(300);

    // Fast path: already ready.
    if let Ok(plate) = state.user_repo.get_ready_character_plate_by_user_id(user_id).await {
        return Some(plate);
    }

    // Self-heal: make sure a generation is in flight before we start polling.
    // Idempotent — if onboarding or scenario_planner already claimed one,
    // `claimed=false` and we reuse it.
    match state.user_repo.get_primary_photo_by_user_id(user_id).await {
        Ok(photo) => match state
            .user_repo
            .claim_character_plate_generation(user_id, photo.id, CHARACTER_PLATE_PROMPT)
            .await
        {
            Ok((plate, claimed)) => {
                if claimed {
                    tracing::info!(
                        user_id = %user_id,
                        plate_id = %plate.id,
                        "flash_generation: claimed character plate (self-heal)"
                    );
                    if let Err(e) = state
                        .job_client
                        .insert(
                            KIND_CHARACTER_PLATE,
                            &CharacterPlateArgs {
                                user_id,
                                source_photo_id: photo.id,
                                plate_id: plate.id,
                            },
                        )
                        .await
                    {
                        tracing::warn!(user_id = %user_id, error = ?e, "flash_generation: failed to enqueue self-heal plate job");
                        return None;
                    }
                }
            }
            Err(e) => {
                tracing::warn!(user_id = %user_id, error = ?e, "flash_generation: claim_character_plate_generation failed");
                return None;
            }
        },
        Err(e) => {
            tracing::warn!(user_id = %user_id, error = ?e, "flash_generation: no primary photo — can't wait for character palette");
            return None;
        }
    }

    tracing::info!(user_id = %user_id, "flash_generation: waiting for character palette to become ready");
    let deadline = std::time::Instant::now() + MAX_WAIT;
    loop {
        if let Ok(plate) = state.user_repo.get_ready_character_plate_by_user_id(user_id).await {
            tracing::info!(user_id = %user_id, plate_id = %plate.id, "flash_generation: character palette ready");
            return Some(plate);
        }
        if std::time::Instant::now() >= deadline {
            tracing::warn!(user_id = %user_id, timeout_secs = MAX_WAIT.as_secs(), "flash_generation: timed out waiting for character palette");
            return None;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

async fn load_simulation_context(
    state: &AppState,
    sim: &models::DecisionSimulation,
) -> Result<SimulationContext> {
    let profile = state.user_repo.get_profile_by_user_id(sim.user_id).await?;
    let story = state.user_repo.get_life_story_by_user_id(sim.user_id).await.ok();
    let life_state = state
        .user_repo
        .build_life_state(sim.user_id)
        .await
        .unwrap_or_else(|_| models::LifeState::default_state());
    let decision = state.decision_repo.get_decision_by_id(sim.decision_id).await.ok();

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
    ctx.life_story = story;
    ctx.decision = decision.clone();
    ctx.time_horizon_months = decision.map(|d| d.time_horizon_months).unwrap_or(12);
    ctx.video_clip_duration_secs = state.cfg.simulation_video_clip_duration_secs;
    if state.cfg.is_development() && state.cfg.scenario_planner_dev_phase_count > 0 {
        ctx.scenario_planner_exact_phases = state.cfg.scenario_planner_dev_phase_count;
    }
    Ok(ctx)
}
