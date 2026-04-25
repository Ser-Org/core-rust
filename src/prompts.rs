//! Prompt builder. Produces system + user prompts for each generation task.
//!
//! Prompts are inlined as Rust string literals — the frontend never sees the
//! prompt text, only the JSON the model returns. The earlier Go implementation
//! used text/template files; those are gone and this module is authoritative.

use crate::models::{self, Decision, LifeState, LifeStory, UserProfile};
use crate::utils::truncate_to_byte_boundary as trunc_byte_boundary;
use serde::Serialize;
use serde_json::{json, Value as JsonValue};

pub const TASK_DASHBOARD: &str = "dashboard";
pub const TASK_CINEMATIC_PROMPT: &str = "cinematic_prompt";
pub const TASK_SCENARIO_PLANNER: &str = "scenario_planner";
pub const TASK_ASSUMPTION_EXTRACTION: &str = "assumption_extraction";
pub const TASK_LIFE_STATE_EXTRACTION: &str = "life_state_extraction";
pub const TASK_ASSUMPTION_CALIBRATION: &str = "assumption_calibration";
pub const TASK_PIPELINE_PROMPT: &str = "pipeline_prompt";

#[derive(Debug, Clone, Default, Serialize)]
pub struct SimulationContext {
    pub user: Option<UserProfile>,
    pub life_state: LifeState,
    pub life_story: Option<LifeStory>,
    pub extracted_context: JsonValue,
    pub photo_url: String,
    pub decision: Option<Decision>,
    pub time_horizon_months: i32,
    pub reference_data: JsonValue,
    pub scenario_plan_path_a_label: String,
    pub scenario_plan_path_a_summary: String,
    pub scenario_plan_path_b_label: String,
    pub scenario_plan_path_b_summary: String,
    pub scenario_planner_exact_phases: i32,
    pub video_clip_duration_secs: i32,
    pub behavioral_profile: models::BehavioralProfile,
    pub financial_profile: models::FinancialProfile,
    pub life_context_profile: models::LifeContextProfile,
    pub financial_fact_sheet: Option<models::FinancialFactSheet>,
    pub assumption_overrides: JsonValue,
}

impl SimulationContext {
    pub fn has_decision(&self) -> bool {
        self.decision
            .as_ref()
            .map(|d| !d.decision_text.is_empty())
            .unwrap_or(false)
    }

    pub fn scenario_planner_min_phases(&self) -> i32 {
        if self.scenario_planner_exact_phases > 0 {
            self.scenario_planner_exact_phases
        } else {
            3
        }
    }

    pub fn scenario_planner_max_phases(&self) -> i32 {
        if self.scenario_planner_exact_phases > 0 {
            self.scenario_planner_exact_phases
        } else {
            3
        }
    }

    pub fn scenario_planner_clip_duration_secs(&self) -> i32 {
        if self.video_clip_duration_secs > 0 {
            self.video_clip_duration_secs
        } else {
            6
        }
    }
}

pub struct PromptBuilder;

impl PromptBuilder {
    pub fn new() -> Self {
        Self
    }

    pub fn build_text_prompt(&self, task_type: &str, ctx: &SimulationContext) -> (String, String) {
        match task_type {
            TASK_DASHBOARD => build_dashboard(ctx),
            TASK_CINEMATIC_PROMPT => build_cinematic_prompt(ctx),
            TASK_SCENARIO_PLANNER => build_scenario_planner(ctx),
            TASK_ASSUMPTION_EXTRACTION => build_assumption_extraction(ctx),
            TASK_LIFE_STATE_EXTRACTION => build_life_state_extraction(ctx),
            TASK_ASSUMPTION_CALIBRATION => build_assumption_calibration(ctx),
            TASK_PIPELINE_PROMPT => build_pipeline_prompt(ctx),
            _ => ("You are a helpful assistant.".into(), "Return {}".into()),
        }
    }
}

fn user_context_summary(ctx: &SimulationContext) -> String {
    let mut out = String::new();
    let life = &ctx.life_state;
    if life.age > 0 {
        out.push_str(&format!("Age: {}\n", life.age));
    } else if !life.age_range.is_empty() {
        out.push_str(&format!("Age range: {}\n", life.age_range));
    }
    if !life.location.is_empty() {
        out.push_str(&format!("Location: {}\n", life.location));
    }
    if !life.gender.is_empty() {
        let g = life.gender.to_ascii_lowercase();
        let pronoun_hint = match g.as_str() {
            "male" | "m" | "man" => {
                " (use he/him pronouns and masculine references like \"the man\")"
            }
            "female" | "f" | "woman" => {
                " (use she/her pronouns and feminine references like \"the woman\")"
            }
            "non_binary" | "nonbinary" | "non-binary" | "nb" | "enby" => {
                " (use they/them pronouns and gender-neutral references like \"the person\")"
            }
            "prefer_not_to_say" | "unspecified" | "unknown" => {
                " (use they/them pronouns and gender-neutral references like \"the person\")"
            }
            _ => "",
        };
        out.push_str(&format!("Gender: {}{}\n", life.gender, pronoun_hint));
    }
    if !life.profession.is_empty() {
        out.push_str(&format!("Profession: {}\n", life.profession));
    } else if !life.role.is_empty() {
        out.push_str(&format!("Role: {}\n", life.role));
    }
    if !life.industry.is_empty() {
        out.push_str(&format!("Industry: {}\n", life.industry));
    }
    if life.income > 0.0 {
        out.push_str(&format!("Annual income: ${:.0}\n", life.income));
    }
    if life.net_worth != 0.0 {
        out.push_str(&format!("Net worth: ${:.0}\n", life.net_worth));
    }
    if life.debt > 0.0 {
        out.push_str(&format!("Debt: ${:.0}\n", life.debt));
    }
    if !life.relationship_status.is_empty() && life.relationship_status != "unknown" {
        out.push_str(&format!("Relationship: {}\n", life.relationship_status));
    }
    if let Some(has) = life.has_dependents {
        out.push_str(&format!(
            "Has dependents: {} ({})\n",
            has, life.dependent_count
        ));
    }
    if !life.goals.is_empty() {
        out.push_str(&format!("Goals: {}\n", life.goals.join(", ")));
    }
    if let Some(story) = &ctx.life_story {
        if !story.ai_summary.is_empty() {
            out.push_str(&format!("\nLife context summary:\n{}\n", story.ai_summary));
        } else if !story.raw_input.is_empty() {
            out.push_str(&format!("\nLife story:\n{}\n", story.raw_input));
        }
    }
    out
}

/// Cap for `rich_user_context_summary` output. Keeps the LLM prompt lean.
const RICH_CONTEXT_MAX_BYTES: usize = 1500;

/// Cap for the life-story summary segment specifically. Bounded so it can't
/// push the structured sections (Behavioral / Financial posture / Life
/// context / Extracted highlights) off the end of the byte budget.
const LIFE_STORY_MAX_BYTES: usize = 600;

/// Builds a planner-facing summary that prioritizes structured profile data
/// (the whole point of Layer A) over the long, variable life-story prose.
/// Sections are appended in priority order; the life story is bounded
/// independently so it cannot crowd out earlier sections under any input.
///
/// Behavioral / Life context / Financial posture sections read from raw
/// `ctx.user` Option fields — `resolve_behavioral_profile` fills "moderate"
/// defaults that look like real user data; we deliberately do NOT use those
/// here because they'd lie to the planner.
fn rich_user_context_summary(ctx: &SimulationContext) -> String {
    let mut out = String::new();

    // 1. Demographics — pronoun cue is critical for downstream prompts.
    out.push_str(&demographics_section(&ctx.life_state));

    // 2. Profession / role / industry.
    out.push_str(&profession_section(&ctx.life_state));

    // 3. Behavioral — only fields the user explicitly set.
    if let Some(profile) = ctx.user.as_ref() {
        out.push_str(&behavioral_section(profile));
    }

    // 4. Life context — only when set.
    if let Some(profile) = ctx.user.as_ref() {
        out.push_str(&life_context_section(profile));
    }

    // 5. Financial posture — qualitative + selected fact-sheet figures.
    if let Some(profile) = ctx.user.as_ref() {
        out.push_str(&financial_posture_section(
            profile,
            ctx.financial_fact_sheet.as_ref(),
        ));
    }

    // 6. Extracted highlights from life-story analysis.
    out.push_str(&extracted_highlights_section(&ctx.extracted_context));

    // 7. Goals.
    if !ctx.life_state.goals.is_empty() {
        out.push_str(&format!("Goals: {}\n", ctx.life_state.goals.join(", ")));
    }

    // 8. Numeric financials — diffusion can't render these anyway, so they
    //    sit below qualitative posture fields. Trimmed before life story if
    //    needed (they're short).
    out.push_str(&numeric_financial_section(&ctx.life_state));

    // 9. Life-story summary — longest variable section. Bounded by both the
    //    overall budget AND a per-section cap so an outsized story can't
    //    crowd out earlier structured sections.
    let remaining = RICH_CONTEXT_MAX_BYTES.saturating_sub(out.len());
    let story_budget = remaining.min(LIFE_STORY_MAX_BYTES);
    if story_budget > 80 {
        if let Some(story) = &ctx.life_story {
            let raw = if !story.ai_summary.is_empty() {
                story.ai_summary.as_str()
            } else {
                story.raw_input.as_str()
            };
            if !raw.is_empty() {
                let label = "\nLife context summary:\n";
                let body_budget = story_budget.saturating_sub(label.len() + 5);
                let body = trunc_byte_boundary(raw, body_budget);
                out.push_str(label);
                out.push_str(body);
                if body.len() < raw.len() {
                    out.push('…');
                }
                out.push('\n');
            }
        }
    }

    // Final safety cap. Should never trigger (sections above respect budgets),
    // but cheap insurance against future sections leaking past the limit.
    if out.len() > RICH_CONTEXT_MAX_BYTES {
        let trimmed = trunc_byte_boundary(&out, RICH_CONTEXT_MAX_BYTES);
        return format!("{}…", trimmed);
    }
    out
}

fn demographics_section(life: &LifeState) -> String {
    let mut s = String::new();
    if life.age > 0 {
        s.push_str(&format!("Age: {}\n", life.age));
    } else if !life.age_range.is_empty() {
        s.push_str(&format!("Age range: {}\n", life.age_range));
    }
    if !life.location.is_empty() {
        s.push_str(&format!("Location: {}\n", life.location));
    }
    if !life.gender.is_empty() {
        let g = life.gender.to_ascii_lowercase();
        let pronoun_hint = match g.as_str() {
            "male" | "m" | "man" => {
                " (use he/him pronouns and masculine references like \"the man\")"
            }
            "female" | "f" | "woman" => {
                " (use she/her pronouns and feminine references like \"the woman\")"
            }
            "non_binary" | "nonbinary" | "non-binary" | "nb" | "enby" | "prefer_not_to_say"
            | "unspecified" | "unknown" => {
                " (use they/them pronouns and gender-neutral references like \"the person\")"
            }
            _ => "",
        };
        s.push_str(&format!("Gender: {}{}\n", life.gender, pronoun_hint));
    }
    s
}

fn profession_section(life: &LifeState) -> String {
    let mut s = String::new();
    if !life.profession.is_empty() {
        s.push_str(&format!("Profession: {}\n", life.profession));
    } else if !life.role.is_empty() {
        s.push_str(&format!("Role: {}\n", life.role));
    }
    if !life.industry.is_empty() {
        s.push_str(&format!("Industry: {}\n", life.industry));
    }
    s
}

fn behavioral_section(profile: &UserProfile) -> String {
    let mut parts: Vec<String> = Vec::new();
    push_if_set(&mut parts, "risk", &profile.risk_tolerance);
    push_if_set(&mut parts, "follow_through", &profile.follow_through);
    push_if_set(&mut parts, "optimism", &profile.optimism_bias);
    push_if_set(&mut parts, "stress", &profile.stress_response);
    push_if_set(&mut parts, "decision", &profile.decision_style);
    if parts.is_empty() {
        String::new()
    } else {
        format!("Behavioral: {}\n", parts.join(", "))
    }
}

fn life_context_section(profile: &UserProfile) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(rs) = profile
        .relationship_status
        .as_deref()
        .filter(|s| !s.is_empty() && *s != "unknown")
    {
        parts.push(format!("relationship={}", rs));
    }
    if let Some(his) = profile
        .household_income_structure
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        parts.push(format!("household={}", his));
    }
    if let Some(dc) = profile.dependent_count {
        if dc > 0 {
            parts.push(format!("dependents={}", dc));
        }
    }
    if let Some(ls) = profile.life_stability.as_deref().filter(|s| !s.is_empty()) {
        parts.push(format!("life_stability={}", ls));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("Life context: {}\n", parts.join(", "))
    }
}

fn financial_posture_section(
    profile: &UserProfile,
    fact_sheet: Option<&models::FinancialFactSheet>,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    push_if_set(&mut parts, "saving", &profile.saving_habits);
    push_if_set(&mut parts, "debt_comfort", &profile.debt_comfort);
    push_if_set(&mut parts, "housing_stability", &profile.housing_stability);
    push_if_set(&mut parts, "income_stability", &profile.income_stability);
    if let Some(fs) = fact_sheet {
        if fs.monthly_income.value > 0.0 {
            parts.push(format!("monthly_income=${:.0}", fs.monthly_income.value));
        }
        if fs.monthly_savings.value > 0.0 {
            parts.push(format!("monthly_savings=${:.0}", fs.monthly_savings.value));
        }
        if fs.runway_months.value > 0.0 {
            parts.push(format!("runway_months={:.0}", fs.runway_months.value));
        }
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("Financial posture: {}\n", parts.join(", "))
    }
}

fn extracted_highlights_section(v: &JsonValue) -> String {
    let mut ext_parts: Vec<String> = Vec::new();
    for key in &[
        "lifestyle",
        "aesthetic_preferences",
        "wardrobe",
        "environment_hints",
    ] {
        if let Some(val) = extracted_str(v, key) {
            ext_parts.push(format!("{}={}", key, val));
        }
    }
    if ext_parts.is_empty() {
        String::new()
    } else {
        format!("Extracted highlights: {}\n", ext_parts.join("; "))
    }
}

fn numeric_financial_section(life: &LifeState) -> String {
    let mut s = String::new();
    if life.income > 0.0 {
        s.push_str(&format!("Annual income: ${:.0}\n", life.income));
    }
    if life.net_worth != 0.0 {
        s.push_str(&format!("Net worth: ${:.0}\n", life.net_worth));
    }
    if life.debt > 0.0 {
        s.push_str(&format!("Debt: ${:.0}\n", life.debt));
    }
    s
}

fn push_if_set(parts: &mut Vec<String>, label: &str, val: &Option<String>) {
    if let Some(v) = val.as_deref().filter(|s| !s.is_empty()) {
        parts.push(format!("{}={}", label, v));
    }
}

/// True when `assumption_overrides` carries something meaningful for the
/// scenario planner to weight (non-null, non-empty object/array).
fn has_assumption_overrides(v: &JsonValue) -> bool {
    match v {
        JsonValue::Null => false,
        JsonValue::Object(o) => !o.is_empty(),
        JsonValue::Array(a) => !a.is_empty(),
        _ => true,
    }
}

// ---------------------------------------------------------------------------
// Layer B — Stage 1 Flux prompt assembly with user-state context
//
// Stage 1 (Flux 2 Max) composes the per-phase scene image. Verbose user-state
// context can ride here without bloating Stage 2 (Runway gen4.5) prompts;
// Runway only ever sees the planner's terse motion text.
//
// Drift prevention strategy:
// 1. Wardrobe lives ONLY in `build_user_state_block` (per-phase, scene-aware).
//    The character plate stays wardrobe-free so it doesn't fight scene context.
// 2. The opening prefix and the closing PIPELINE_IDENTITY_LOCK_SUFFIX both
//    reinforce "preserve face from reference image". Diffusion attention
//    weakens in the middle of long prompts; reinforcing at both ends counters
//    that drift vector.
// ---------------------------------------------------------------------------

/// Opening clause for Stage 1 prompts. Establishes that the reference image
/// is the identity anchor and the rest of the prompt describes the scene.
const PIPELINE_SCENE_PREFIX: &str = "Place the subject from the reference image into the scene described. Preserve their exact facial features, hair, skin tone, and build — this is the same person. The subject faces the camera directly (forward-facing, head and shoulders squared to the lens); do not render profile or back views.\n\n";

/// Cinematic style/lens language. Inherited by Runway's motion stage so it
/// doesn't have to reinvent the composition.
const PIPELINE_SCENE_SUFFIX: &str = "\n\nCinematic photograph, shot on Sony A7 IV with a 35mm f/1.4 lens, available light, 4K ultra-detailed. Natural skin with visible pore texture and subtle asymmetry — reject smooth \"AI face\" look. Anatomically correct hands with five visible fingers. Documentary prestige-film aesthetic, Kodak Portra 800 palette. Pure photograph; no illustration or stylization.";

/// Anti-text/UI/multi-subject constraints — diffusion models render these
/// poorly and any reference to additional people produces broken output.
const PIPELINE_SCENE_CONSTRAINTS: &str = "\n\nConstraints: no visible text, typography, captions, signage, logos, or device screens/UI. Horizontal 16:9 framing. Only the subject appears; no other people in the frame.";

/// Closing identity-lock clause. Repeats preserve-identity at the end of the
/// prompt to counter middle-text attention dilution as the user-state block
/// and scene description grow. Fixed and never trimmed.
const PIPELINE_IDENTITY_LOCK_SUFFIX: &str = "\n\nIdentity lock: the subject's face, hair, skin tone, and build match the reference image exactly. Do not alter their appearance to fit the scene, wardrobe, or context.";

/// Total byte budget for the assembled Stage 1 Flux prompt. Flux is generous
/// (~2 KB is comfortable). Stage 2 Runway has a tighter ~1 KB cap, but its
/// prompt path is unrelated and unaffected here.
pub const PIPELINE_PROMPT_MAX_BYTES: usize = 2000;

/// Cap on the user-state block alone. Keeps the Flux prompt scene-led rather
/// than profile-led — over-prescription causes Flux to over-anchor on
/// wardrobe/environment and fight the per-phase scene description.
const USER_STATE_MAX_BYTES: usize = 400;

/// Assembles the final Stage 1 Flux prompt:
///
/// `[PREFIX][middle][SUFFIX][CONSTRAINTS][IDENTITY_LOCK_SUFFIX]`
///
/// where `middle` is `"Subject context: <user_state_block>. Scene: <scene_prompt>"`
/// when the block is non-empty, or just `"Scene: <scene_prompt>"` otherwise.
/// The variable middle is byte-trimmed so the assembled string fits within
/// `PIPELINE_PROMPT_MAX_BYTES`. The fixed tail (suffix + constraints + identity
/// lock) is never trimmed — preserve-identity reinforcement must always reach
/// the end of the prompt.
pub fn build_flux_scene_prompt(user_state_block: &str, scene_prompt: &str) -> String {
    let tail = format!(
        "{}{}{}",
        PIPELINE_SCENE_SUFFIX, PIPELINE_SCENE_CONSTRAINTS, PIPELINE_IDENTITY_LOCK_SUFFIX
    );
    let middle = if user_state_block.is_empty() {
        format!("Scene: {}", scene_prompt)
    } else {
        format!(
            "Subject context: {}. Scene: {}",
            user_state_block, scene_prompt
        )
    };
    let fixed_len = PIPELINE_SCENE_PREFIX.len() + tail.len();
    let middle_budget = PIPELINE_PROMPT_MAX_BYTES.saturating_sub(fixed_len);
    let middle_trimmed = trunc_byte_boundary(&middle, middle_budget);
    format!("{}{}{}", PIPELINE_SCENE_PREFIX, middle_trimmed, tail)
}

/// Builds a short prose subject-description block for Flux Stage 1. Phrase-form
/// (NOT JSON), no raw numbers (diffusion can't render "$184k"). Drives the
/// scene composition toward an environment, wardrobe, and bearing that match
/// the user's life-state without prescribing facial features.
///
/// Capped at `USER_STATE_MAX_BYTES`. The returned string is meant to be
/// inserted into the Flux prompt as `"Subject context: <block>. Scene: ..."`.
pub fn build_user_state_block(ctx: &SimulationContext) -> String {
    let life = &ctx.life_state;
    let mut parts: Vec<String> = Vec::new();

    let age = age_phrase(life);
    let prof = profession_phrase(life);
    if !age.is_empty() && !prof.is_empty() {
        parts.push(format!("{} {}", age, prof));
    } else if !age.is_empty() {
        parts.push(age);
    } else if !prof.is_empty() {
        parts.push(prof);
    }

    // Location — cheap, high-signal discriminator (Brooklyn vs Singapore)
    // that Stage 1 would otherwise lose when scene_prompt is generic. Gender
    // is deliberately NOT projected here — photo + plate carry that signal,
    // and adding it to the subject context risks drift without adding value.
    if !life.location.is_empty() {
        parts.push(format!("based in {}", life.location));
    }

    // Bearing — read raw `ctx.user` Option fields. Defaults from
    // `resolve_behavioral_profile` ("analytical" / "deliberate") are NOT real
    // user signal and must not be projected onto the diffusion subject for
    // users we know nothing about. Empty string means "let the scene_prompt
    // and character plate carry the subject description".
    let bearing = ctx
        .user
        .as_ref()
        .map(|p| bearing_phrase(p.stress_response.as_deref(), p.decision_style.as_deref()))
        .unwrap_or_default();
    if !bearing.is_empty() {
        parts.push(bearing);
    }

    if let Some(s) = extracted_str(&ctx.extracted_context, "aesthetic_preferences") {
        parts.push(format!("{} aesthetic", s));
    }
    if let Some(s) = extracted_str(&ctx.extracted_context, "wardrobe") {
        parts.push(s);
    }
    if let Some(s) = extracted_str(&ctx.extracted_context, "lifestyle") {
        parts.push(s);
    }
    if let Some(s) = extracted_str(&ctx.extracted_context, "environment_hints") {
        parts.push(s);
    }

    match ctx.financial_profile.housing_stability.as_str() {
        "stable" => parts.push("settled lifestyle".to_string()),
        "unstable" => parts.push("in transition".to_string()),
        _ => {}
    }

    let raw = parts.join("; ");
    if raw.len() > USER_STATE_MAX_BYTES {
        let trimmed = trunc_byte_boundary(&raw, USER_STATE_MAX_BYTES);
        return format!("{}…", trimmed);
    }
    raw
}

fn age_phrase(life: &LifeState) -> String {
    if life.age > 0 {
        let a = life.age;
        if a < 20 {
            return "late teens".into();
        }
        if a < 25 {
            return "early 20s".into();
        }
        if a < 30 {
            return "late 20s".into();
        }
        if a < 35 {
            return "early 30s".into();
        }
        if a < 40 {
            return "mid-30s".into();
        }
        if a < 45 {
            return "early 40s".into();
        }
        if a < 50 {
            return "mid-40s".into();
        }
        if a < 55 {
            return "early 50s".into();
        }
        if a < 60 {
            return "mid-50s".into();
        }
        if a < 65 {
            return "late 50s to early 60s".into();
        }
        return "in their 60s".into();
    }
    if !life.age_range.is_empty() {
        return match life.age_range.as_str() {
            "18-24" => "early 20s".into(),
            "25-34" => "late 20s to early 30s".into(),
            "35-44" => "late 30s to early 40s".into(),
            "45-54" => "late 40s to early 50s".into(),
            "55-64" => "late 50s to early 60s".into(),
            "65+" => "in their late 60s".into(),
            other => other.to_string(),
        };
    }
    String::new()
}

fn profession_phrase(life: &LifeState) -> String {
    if !life.profession.is_empty() {
        return life.profession.clone();
    }
    if !life.role.is_empty() {
        return life.role.clone();
    }
    if !life.industry.is_empty() {
        let pretty = match life.industry.as_str() {
            "office_tech" => "tech professional",
            "creative_media" => "creative professional",
            "healthcare" => "healthcare professional",
            "trades_physical" => "trades professional",
            "hospitality" => "hospitality professional",
            "education" => "educator",
            "finance_law" => "finance/legal professional",
            other => other,
        };
        return pretty.to_string();
    }
    String::new()
}

/// Maps explicit stress + decision-style signals to a short bearing phrase
/// for the Flux subject description. Returns empty unless BOTH inputs are
/// `Some` and non-empty — absent behavioral data is silence, not a default.
fn bearing_phrase(stress: Option<&str>, style: Option<&str>) -> String {
    let stress = match stress.filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => return String::new(),
    };
    let style = match style.filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => return String::new(),
    };
    match (stress, style) {
        ("analytical", "deliberate") => "calm analytical bearing".into(),
        ("analytical", "intuitive") => "thoughtful, alert bearing".into(),
        ("analytical", _) => "measured bearing".into(),
        ("emotional", "deliberate") => "warm, considered bearing".into(),
        ("emotional", "intuitive") => "expressive, intuitive presence".into(),
        ("emotional", _) => "expressive bearing".into(),
        ("withdrawn", _) => "reserved, contained bearing".into(),
        _ => String::new(),
    }
}

fn extracted_str(v: &JsonValue, key: &str) -> Option<String> {
    let val = v.get(key)?;
    match val {
        JsonValue::String(s) if !s.is_empty() => Some(s.clone()),
        JsonValue::Array(arr) => {
            let parts: Vec<String> = arr
                .iter()
                .filter_map(|x| x.as_str().filter(|s| !s.is_empty()).map(|s| s.to_string()))
                .collect();
            if parts.is_empty() {
                None
            } else {
                Some(parts.join(", "))
            }
        }
        _ => None,
    }
}

fn build_dashboard(ctx: &SimulationContext) -> (String, String) {
    let sys = r#"You are a life-modeling narrator. Produce a dashboard snapshot summarizing the user's current trajectory.

Output JSON:
{
  "life_quality_trajectory": {"wellbeing_curve": [{"month": N, "value": 0-100, "label": "..."}], "fulfillment_curve": [...]},
  "life_momentum_score": {"score": 0-100, "justification": "..."},
  "probability_outlook": {"best": 0.0-1.0, "likely": 0.0-1.0, "worst": 0.0-1.0},
  "narrative_summary": "..."
}"#;
    let uc = user_context_summary(ctx);
    let user = format!(
        "User context:\n{}\n\nProduce a grounded dashboard snapshot. Return JSON only.",
        uc
    );
    (sys.into(), user)
}

fn build_scenario_planner(ctx: &SimulationContext) -> (String, String) {
    let phases = ctx.scenario_planner_min_phases();
    let clip_dur = ctx.scenario_planner_clip_duration_secs();
    let sys = format!(
        r#"You are a cinematic scenario planner. Produce two divergent future paths (A and B) for the user's decision, each with exactly {phases} phases representing temporal milestones. Each phase produces one {clip_dur}-second cinematic video clip.

For each phase provide:
- index (0, 1, ..., {last})
- title (short)
- time_label (e.g. "Month 1")
- scene_prompt — a 1–3 sentence photorealistic description of the composed shot: subject in their environment, what they're doing, framing, and any camera/lens language (focal length, depth of field, shot type). Describe only the first frame, not change over time.
- motion_prompt — only what moves during the clip: subject action (turning, walking, gesturing) and camera motion (slow push-in, pan left, static hold). Do NOT restate composition, camera/lens specs, or subject appearance — those belong in scene_prompt.
- edit_prompt — color grade / lighting / atmosphere adjustments
- narration_text — a short poetic narration line for this phase
- overlay_data: {{"primary_metric": {{"label":"Savings","value":"$32k down","trend":"down"}}, "secondary_metric": {{...}}, "mood_tag":"tense", "risk_level":"medium"}}

Hard constraints on every scene_prompt and motion_prompt:
- Do NOT describe phone screens, laptop screens, monitors, TVs, or any device UI. Diffusion models render these poorly.
- Do NOT describe visible text, signage, captions, logos, or labels. Same reason.
- If a beat conceptually involves a device (e.g. reading a message, checking a notification, doing work on a laptop), frame it indirectly — describe the subject's posture, gaze, expression, or reaction. Never the screen content.
- The subject is the ONLY person in every scene. Never describe other people, crowds, audiences, families, partners, colleagues, pedestrians, baristas, coworkers, children, or background figures. No people inside reflections, photos, posters, or framed pictures either. The downstream image and video models render exactly one human; any reference to additional people produces broken output.
- If a beat conceptually involves another person (a conversation, a wedding, a meeting, a family dinner, a presentation, a date, a goodbye), reframe it as a solo moment — show the subject before or after the interaction, alone in the space, doing the preparation or carrying the aftermath, walking away from the venue, looking at an empty seat across the table, or sitting alone in the room someone has just left. Pick beats that are plausibly solitary in the first place.
- Pronouns and gendered references must match the user's gender from the context. If Gender is male, use he/him and words like "the man"; if female, use she/her and "the woman"; if non-binary or unspecified, use they/them and "the person". Never mix pronouns for the same subject across a scene or across paths.

Output JSON:
{{
  "path_a": {{"label":"...", "outcome":"path_a", "phases":[...]}},
  "path_b": {{"label":"...", "outcome":"path_b", "phases":[...]}},
  "shared_context": {{"decision_theme":"...", "time_horizon_months": N}}
}}"#,
        phases = phases,
        clip_dur = clip_dur,
        last = phases - 1
    );
    let uc = rich_user_context_summary(ctx);
    let decision_text = ctx
        .decision
        .as_ref()
        .map(|d| d.decision_text.clone())
        .unwrap_or_default();
    let overrides_section = if has_assumption_overrides(&ctx.assumption_overrides) {
        format!(
            "\n\nUser-corrected assumptions (the user has edited these — weight them heavily over any inferred defaults):\n{}",
            serde_json::to_string(&ctx.assumption_overrides).unwrap_or_default()
        )
    } else {
        String::new()
    };
    let user = format!(
        "User context:\n{}\n\nDecision: {}\n\nTime horizon: {} months.{}\n\nReturn JSON only.",
        uc, decision_text, ctx.time_horizon_months, overrides_section
    );
    (sys, user)
}

fn build_assumption_extraction(ctx: &SimulationContext) -> (String, String) {
    let sys = r#"You are an assumption analyst. Extract the CRITICAL editable assumptions and risks from the simulation outputs. Assumptions must be concrete, testable statements the user could correct.

For each editable assumption that maps to a stable user attribute, set `profile_field` to exactly one of:
  estimated_net_worth, estimated_yearly_salary, risk_tolerance, follow_through,
  optimism_bias, stress_response, decision_style, saving_habits, debt_comfort,
  housing_stability, income_stability, liquid_net_worth_source, relationship_status,
  household_income_structure, dependent_count, life_stability
If the assumption is not a stable user attribute (e.g. a prediction about the future), use null.

Output JSON:
{
  "assumptions": [
    {"description": "...", "confidence": 0.0-1.0, "source": "user_input|life_state|reference_data|inferred", "kind": "premise|prediction", "grounding": "stated|derived|inferred|speculative", "category": "financial|behavioral|environmental|social|health|career|lifestyle", "editable": true|false, "evidence_refs": [], "profile_field": "risk_tolerance|null"}
  ],
  "risks": [
    {"description": "...", "likelihood": "low|medium|high", "impact": "low|medium|high", "category": "financial|health|career|relationship|lifestyle", "linked_assumption_ids": [], "mitigation_hint": "..."}
  ]
}"#;
    let uc = user_context_summary(ctx);
    let user = format!(
        "User context:\n{}\n\nPath A ({}):\n{}\n\nPath B ({}):\n{}\n\nExtract assumptions and risks. Return JSON only.",
        uc,
        ctx.scenario_plan_path_a_label,
        ctx.scenario_plan_path_a_summary,
        ctx.scenario_plan_path_b_label,
        ctx.scenario_plan_path_b_summary
    );
    (sys.into(), user)
}

fn build_life_state_extraction(ctx: &SimulationContext) -> (String, String) {
    let sys = r#"You extract structured life-state fields from the user's narrative. Return ONLY the fields you can confidently determine from the text. Do not invent.

Output JSON (keys optional; omit when unknown):
{
  "age": 28,
  "location": "San Francisco",
  "gender": "male|female|non_binary|prefer_not_to_say",
  "education_level": "...",
  "industry": "...",
  "role": "...",
  "profession": "...",
  "career_experience_yr": 5,
  "debt": 0,
  "risk_tolerance": "low|medium|high",
  "health_score": 0.0-1.0,
  "network_strength": "low|medium|high",
  "ambition": "low|medium|high",
  "stress_level": "low|medium|high",
  "relationship_status": "single|partnered|married|divorced|widowed",
  "has_dependents": true|false,
  "dependent_count": 0,
  "goals": ["..."],
  "geographic_mobility": "low|medium|high",
  "monthly_spending": 0,
  "monthly_savings": 0,
  "housing_cost": 0
}"#;
    let raw = ctx
        .life_story
        .as_ref()
        .map(|s| s.raw_input.clone())
        .unwrap_or_default();
    let user = format!("Life story:\n{}\n\nReturn JSON only.", raw);
    (sys.into(), user)
}

fn build_assumption_calibration(ctx: &SimulationContext) -> (String, String) {
    let sys = r#"You refine Scout's durable user state from assumption overrides. Given the current profile, life story, and user-corrected assumptions, output a sparse profile patch, extracted_context updates, and refreshed summary.

Output JSON:
{
  "profile_updates": {"estimated_net_worth": 0, "risk_tolerance": "..."},
  "extracted_context_updates": {},
  "extracted_context_clears": [],
  "calibration_summary": "...",
  "calibration_notes": ["..."],
  "refreshed_ai_summary": "..."
}"#;
    let uc = user_context_summary(ctx);
    let user = format!(
        "User context:\n{}\n\nAssumption overrides:\n{}\n\nReturn JSON only.",
        uc,
        serde_json::to_string(&ctx.assumption_overrides).unwrap_or_default()
    );
    (sys.into(), user)
}

fn build_cinematic_prompt(ctx: &SimulationContext) -> (String, String) {
    let sys = r#"You produce short cinematic video prompts for one 6-second clip that represents the user's decision outcome.

Hard constraints on scene_prompt and motion_prompt:
- Do NOT describe phone screens, laptop screens, monitors, TVs, or any device UI. Diffusion models render these poorly.
- Do NOT describe visible text, signage, captions, logos, or labels. Same reason.
- If a beat conceptually involves a device (e.g. reading a message, checking a notification, doing work on a laptop), frame it indirectly — describe the subject's posture, gaze, expression, or reaction. Never the screen content.
- The subject is the ONLY person in the scene. Never describe other people, crowds, audiences, families, partners, colleagues, pedestrians, baristas, coworkers, children, or background figures. No people inside reflections, photos, posters, or framed pictures either. The downstream image and video models render exactly one human; any reference to additional people produces broken output.
- If the beat conceptually involves another person (a conversation, a meeting, a date), reframe it as a solo moment — the subject before or after the interaction, alone in the space, doing the preparation or carrying the aftermath. Pick a beat that is plausibly solitary in the first place.
- Pronouns and gendered references must match the user's gender from the context. If Gender is male, use he/him and words like "the man"; if female, use she/her and "the woman"; if non-binary or unspecified, use they/them and "the person". Never mix pronouns for the same subject.

Output JSON: {"scene_prompt": "...", "motion_prompt": "...", "edit_prompt": "..."}"#;
    let uc = user_context_summary(ctx);
    let user = format!("User context:\n{}\n\nReturn JSON only.", uc);
    (sys.into(), user)
}

fn build_pipeline_prompt(ctx: &SimulationContext) -> (String, String) {
    build_cinematic_prompt(ctx)
}

/// Helper: return a scenario path summary string for a parsed scenario plan path.
pub fn scenario_path_summary(path: &JsonValue) -> String {
    let mut out = String::new();
    if let Some(phases) = path.get("phases").and_then(|v| v.as_array()) {
        for p in phases {
            let title = p.get("title").and_then(|v| v.as_str()).unwrap_or("");
            let scene = p.get("scene_prompt").and_then(|v| v.as_str()).unwrap_or("");
            out.push_str(&format!("- {}: {}\n", title, scene));
        }
    }
    out
}

/// Suggested first decision prompt (called after onboarding completion).
pub fn suggested_first_decision(ctx: &SimulationContext) -> (String, String) {
    let sys = r#"You are Scout. Suggest ONE realistic decision this user might want to simulate first, based on their life context.

Output JSON:
{"decision_text": "...", "time_horizon_months": 12, "category": "...", "why_it_matters": "..."}"#;
    let uc = user_context_summary(ctx);
    let user = format!("User context:\n{}\n\nReturn JSON only.", uc);
    (sys.into(), user)
}

/// Suggested first what-if prompt.
pub fn suggested_first_what_if(ctx: &SimulationContext) -> (String, String) {
    let sys = r#"You are Scout. Suggest ONE realistic "what if" question this user might explore as a quick Flash vision.

Output JSON: {"question": "What if I moved to Lisbon for a year?", "mood": "optimistic|neutral|tense"}"#;
    let uc = user_context_summary(ctx);
    let user = format!("User context:\n{}\n\nReturn JSON only.", uc);
    (sys.into(), user)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Build a SimulationContext with every field plausibly maxed-out, to
    /// verify byte caps hold against real-world worst cases.
    fn synthetic_max_ctx() -> SimulationContext {
        let mut ctx = SimulationContext::default();
        ctx.life_state.age = 38;
        ctx.life_state.age_range = "35-44".into();
        ctx.life_state.location = "San Francisco, CA, USA".into();
        ctx.life_state.gender = "non_binary".into();
        ctx.life_state.profession = "Senior Staff Software Engineer".into();
        ctx.life_state.role = "Tech Lead".into();
        ctx.life_state.industry = "office_tech".into();
        ctx.life_state.income = 285000.0;
        ctx.life_state.net_worth = 1_240_000.0;
        ctx.life_state.debt = 420_000.0;
        ctx.life_state.relationship_status = "married".into();
        ctx.life_state.has_dependents = Some(true);
        ctx.life_state.dependent_count = 2;
        ctx.life_state.goals = vec![
            "buy a larger home in the next 18 months".into(),
            "transition to founding role within 3 years".into(),
            "establish college funds for both children".into(),
        ];
        // Raw user profile — Layer A reads these Option fields directly to
        // distinguish "user-set" from "default-filled".
        ctx.user = Some(models::UserProfile {
            id: uuid::Uuid::nil(),
            user_id: uuid::Uuid::nil(),
            estimated_net_worth: 1_240_000.0,
            estimated_yearly_salary: 285_000.0,
            onboarding_status: "complete".into(),
            risk_tolerance: Some("moderate".into()),
            follow_through: Some("high".into()),
            optimism_bias: Some("moderate".into()),
            stress_response: Some("analytical".into()),
            decision_style: Some("deliberate".into()),
            saving_habits: Some("consistent_savers".into()),
            debt_comfort: Some("managed".into()),
            housing_stability: Some("stable".into()),
            income_stability: Some("high".into()),
            liquid_net_worth_source: Some("mixed".into()),
            relationship_status: Some("married".into()),
            household_income_structure: Some("dual_primary".into()),
            dependent_count: Some(2),
            life_stability: Some("very_stable".into()),
            onboarding_path: "story".into(),
            age_bracket: Some("35-44".into()),
            gender: Some("non_binary".into()),
            living_situation: Some("own".into()),
            industry: Some("office_tech".into()),
            career_stage: Some("senior".into()),
            net_worth_bracket: Some("established".into()),
            income_bracket: Some("high".into()),
            cinematic_context_completed: true,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        });
        // Resolved profiles — used by Layer B's build_user_state_block.
        ctx.behavioral_profile = models::BehavioralProfile {
            risk_tolerance: "moderate".into(),
            follow_through: "high".into(),
            optimism_bias: "moderate".into(),
            stress_response: "analytical".into(),
            decision_style: "deliberate".into(),
        };
        ctx.financial_profile = models::FinancialProfile {
            saving_habits: "consistent_savers".into(),
            debt_comfort: "managed".into(),
            housing_stability: "stable".into(),
            income_stability: "high".into(),
            liquid_net_worth_fraction: 0.55,
        };
        ctx.life_context_profile = models::LifeContextProfile {
            relationship_status: "married".into(),
            household_income_structure: "dual_primary".into(),
            dependent_count: 2,
            life_stability: "very_stable".into(),
        };
        let mut fs = models::FinancialFactSheet::default();
        fs.monthly_income.value = 23750.0;
        fs.monthly_savings.value = 9100.0;
        fs.runway_months.value = 18.0;
        ctx.financial_fact_sheet = Some(fs);
        ctx.extracted_context = json!({
            "lifestyle": "early-rising endurance athlete; weekend trail runner",
            "aesthetic_preferences": "muted neutrals, textured natural fabrics",
            "wardrobe": "fitted technical layers, minimalist palette",
            "environment_hints": ["light-filled craftsman home", "coastal weather"],
        });
        ctx.life_story = Some(models::LifeStory {
            id: uuid::Uuid::nil(),
            user_id: uuid::Uuid::nil(),
            raw_input: "x".repeat(1200),
            input_method: "text".into(),
            ai_summary: "Long, well-developed life-story summary. ".repeat(20),
            extracted_context: JsonValue::Null,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        });
        ctx.assumption_overrides = json!({
            "estimated_net_worth": 1_500_000,
            "risk_tolerance": "low",
            "housing_stability": "stable",
        });
        ctx
    }

    #[test]
    fn rich_user_context_summary_respects_byte_cap() {
        let ctx = synthetic_max_ctx();
        let out = rich_user_context_summary(&ctx);
        // RICH_CONTEXT_MAX_BYTES is 1500; the cap is enforced via trunc + ellipsis,
        // so the post-truncation string can be up to 1500 + 3 (UTF-8 ellipsis) bytes.
        assert!(
            out.len() <= RICH_CONTEXT_MAX_BYTES + 3,
            "rich_user_context_summary exceeded cap: {} > {}",
            out.len(),
            RICH_CONTEXT_MAX_BYTES + 3
        );
    }

    #[test]
    fn rich_user_context_summary_keeps_structured_sections_under_huge_life_story() {
        // Stress test: pad the life story past the overall cap. The structured
        // sections (Behavioral / Life context / Financial posture / Extracted
        // highlights) MUST still appear — they're the whole point of Layer A.
        let mut ctx = synthetic_max_ctx();
        let story = ctx.life_story.as_mut().unwrap();
        story.ai_summary = "A".repeat(5_000);
        story.raw_input = "B".repeat(5_000);

        let out = rich_user_context_summary(&ctx);
        assert!(
            out.contains("Behavioral:"),
            "missing Behavioral section: {out}"
        );
        assert!(
            out.contains("Life context:"),
            "missing Life context section: {out}"
        );
        assert!(
            out.contains("Financial posture:"),
            "missing Financial posture section: {out}"
        );
        assert!(
            out.contains("Extracted highlights:"),
            "missing Extracted highlights section: {out}"
        );
    }

    #[test]
    fn rich_user_context_summary_omits_behavioral_when_user_unset() {
        // No user profile → no behavioral / life-context / financial-posture
        // sections. We must not project resolved defaults (e.g. "moderate")
        // onto the planner as if they were real signal.
        let mut ctx = synthetic_max_ctx();
        ctx.user = None;
        ctx.life_story = None;
        let out = rich_user_context_summary(&ctx);
        assert!(!out.contains("Behavioral:"), "behavioral leaked: {out}");
        assert!(!out.contains("Life context:"), "life context leaked: {out}");
        assert!(
            !out.contains("Financial posture:"),
            "financial posture leaked: {out}"
        );
    }

    #[test]
    fn rich_user_context_summary_omits_behavioral_when_all_options_none() {
        let mut ctx = synthetic_max_ctx();
        if let Some(profile) = ctx.user.as_mut() {
            profile.risk_tolerance = None;
            profile.follow_through = None;
            profile.optimism_bias = None;
            profile.stress_response = None;
            profile.decision_style = None;
        }
        let out = rich_user_context_summary(&ctx);
        assert!(
            !out.contains("Behavioral:"),
            "behavioral section appeared with no Some fields: {out}"
        );
    }

    #[test]
    fn user_state_block_respects_byte_cap() {
        let ctx = synthetic_max_ctx();
        let out = build_user_state_block(&ctx);
        assert!(
            out.len() <= USER_STATE_MAX_BYTES + 3,
            "build_user_state_block exceeded cap: {} > {}",
            out.len(),
            USER_STATE_MAX_BYTES + 3
        );
    }

    #[test]
    fn flux_scene_prompt_respects_total_budget() {
        // Pass an oversized scene prompt and a maxed user_state_block to confirm
        // the assembled prompt still fits, with the fixed identity-lock tail
        // never trimmed.
        let user_state = "x".repeat(USER_STATE_MAX_BYTES);
        let scene = "y".repeat(5000);
        let out = build_flux_scene_prompt(&user_state, &scene);
        assert!(
            out.len() <= PIPELINE_PROMPT_MAX_BYTES,
            "build_flux_scene_prompt exceeded budget: {} > {}",
            out.len(),
            PIPELINE_PROMPT_MAX_BYTES
        );
        // Identity-lock tail must always be present (never trimmed).
        assert!(
            out.ends_with(PIPELINE_IDENTITY_LOCK_SUFFIX),
            "identity-lock tail was trimmed off"
        );
    }

    #[test]
    fn flux_scene_prompt_omits_subject_context_when_block_empty() {
        let out = build_flux_scene_prompt("", "a quiet kitchen at dawn");
        assert!(
            !out.contains("Subject context:"),
            "Empty user_state_block should not produce a 'Subject context:' label"
        );
        assert!(out.contains("Scene: a quiet kitchen at dawn"));
        assert!(out.ends_with(PIPELINE_IDENTITY_LOCK_SUFFIX));
    }

    #[test]
    fn flux_scene_prompt_includes_subject_context_when_block_present() {
        let out = build_flux_scene_prompt("mid-30s software engineer", "a quiet kitchen at dawn");
        assert!(out.contains("Subject context: mid-30s software engineer"));
        assert!(out.contains("Scene: a quiet kitchen at dawn"));
    }

    #[test]
    fn has_assumption_overrides_recognizes_meaningful_payloads() {
        assert!(!has_assumption_overrides(&JsonValue::Null));
        assert!(!has_assumption_overrides(&json!({})));
        assert!(!has_assumption_overrides(&json!([])));
        assert!(has_assumption_overrides(&json!({"net_worth": 100})));
        assert!(has_assumption_overrides(&json!([1, 2, 3])));
        assert!(has_assumption_overrides(&json!("scalar")));
    }

    #[test]
    fn age_phrase_age_buckets() {
        let mut life = models::LifeState::default();
        life.age = 22;
        assert_eq!(age_phrase(&life), "early 20s");
        life.age = 38;
        assert_eq!(age_phrase(&life), "mid-30s");
        life.age = 67;
        assert_eq!(age_phrase(&life), "in their 60s");
        life.age = 0;
        life.age_range = "45-54".into();
        assert_eq!(age_phrase(&life), "late 40s to early 50s");
    }

    #[test]
    fn bearing_phrase_pairs() {
        assert!(bearing_phrase(Some("analytical"), Some("deliberate")).contains("calm analytical"));
        assert!(bearing_phrase(Some("withdrawn"), Some("deliberate")).contains("reserved"));
        assert_eq!(
            bearing_phrase(Some("unrecognized"), Some("unrecognized")),
            ""
        );
    }

    #[test]
    fn bearing_phrase_returns_empty_when_either_input_absent() {
        // No defaults-as-data — absent stress or style must produce silence.
        assert_eq!(bearing_phrase(None, Some("deliberate")), "");
        assert_eq!(bearing_phrase(Some("analytical"), None), "");
        assert_eq!(bearing_phrase(None, None), "");
        // Empty strings are treated the same as None.
        assert_eq!(bearing_phrase(Some(""), Some("deliberate")), "");
        assert_eq!(bearing_phrase(Some("analytical"), Some("")), "");
    }

    #[test]
    fn user_state_block_omits_bearing_when_user_has_no_behavioral_data() {
        // Lock in the no-defaults-as-data invariant for the visual prompt.
        // A user whose UserProfile has stress_response=None must NOT get
        // "bearing" or "presence" projected into their Flux subject context.
        let mut ctx = synthetic_max_ctx();
        if let Some(profile) = ctx.user.as_mut() {
            profile.stress_response = None;
            profile.decision_style = None;
        }
        let out = build_user_state_block(&ctx);
        assert!(
            !out.contains("bearing"),
            "user_state_block leaked a default bearing for a user with no behavioral data: {out}"
        );
        assert!(
            !out.contains("presence"),
            "user_state_block leaked a default presence for a user with no behavioral data: {out}"
        );
    }

    #[test]
    fn user_state_block_includes_location_when_set() {
        // Location is an easy Stage 1 discriminator — Brooklyn vs Singapore.
        // Losing it would make the visual prompt rely on scene_prompt text
        // alone, which may be generic.
        let ctx = synthetic_max_ctx();
        let out = build_user_state_block(&ctx);
        assert!(
            out.contains("San Francisco"),
            "location missing from user_state_block: {out}"
        );
    }

    #[test]
    fn user_state_block_omits_location_when_unset() {
        let mut ctx = synthetic_max_ctx();
        ctx.life_state.location = String::new();
        let out = build_user_state_block(&ctx);
        assert!(
            !out.contains("based in"),
            "user_state_block emitted 'based in' with empty location: {out}"
        );
    }

    #[test]
    fn user_state_block_omits_bearing_when_ctx_user_is_none() {
        let mut ctx = synthetic_max_ctx();
        ctx.user = None;
        let out = build_user_state_block(&ctx);
        assert!(
            !out.contains("bearing"),
            "user_state_block leaked a bearing when ctx.user is None: {out}"
        );
        assert!(
            !out.contains("presence"),
            "user_state_block leaked a presence when ctx.user is None: {out}"
        );
    }
}
