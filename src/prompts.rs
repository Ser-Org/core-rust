//! Prompt builder. Produces system + user prompts for each generation task.
//!
//! Prompts are inlined as Rust string literals — the frontend never sees the
//! prompt text, only the JSON the model returns. The earlier Go implementation
//! used text/template files; those are gone and this module is authoritative.

use crate::models::{
    self, ClarifyingQuestion, Decision, LifeState, LifeStory, Routine, UserProfile,
};
use serde::Serialize;
use serde_json::{json, Value as JsonValue};

pub const TASK_ONBOARDING_QUESTIONS: &str = "onboarding_questions";
pub const TASK_ROUTINE_INFERENCE: &str = "routine_inference";
pub const TASK_DASHBOARD: &str = "dashboard";
pub const TASK_STRUCTURED_SYNTHESIS: &str = "structured_answers_synthesis";
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
    pub routines: Vec<Routine>,
    pub photo_url: String,
    pub decision: Option<Decision>,
    pub clarifying_qas: Vec<ClarifyingQuestion>,
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

    pub fn build_text_prompt(
        &self,
        task_type: &str,
        ctx: &SimulationContext,
    ) -> (String, String) {
        match task_type {
            TASK_ONBOARDING_QUESTIONS => build_onboarding_questions(ctx),
            TASK_ROUTINE_INFERENCE => build_routine_inference(ctx),
            TASK_DASHBOARD => build_dashboard(ctx),
            TASK_CINEMATIC_PROMPT => build_cinematic_prompt(ctx),
            TASK_SCENARIO_PLANNER => build_scenario_planner(ctx),
            TASK_ASSUMPTION_EXTRACTION => build_assumption_extraction(ctx),
            TASK_LIFE_STATE_EXTRACTION => build_life_state_extraction(ctx),
            TASK_STRUCTURED_SYNTHESIS => build_structured_synthesis(ctx),
            TASK_ASSUMPTION_CALIBRATION => build_assumption_calibration(ctx),
            TASK_PIPELINE_PROMPT => build_pipeline_prompt(ctx),
            _ => (
                "You are a helpful assistant.".into(),
                "Return {}".into(),
            ),
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

fn build_onboarding_questions(ctx: &SimulationContext) -> (String, String) {
    let sys = r#"You are a life modeling analyst for Scout, a life decision intelligence platform. Given a user's life story and a specific decision they want to simulate, your tasks are:
1. Generate 3–5 high-leverage clarifying questions specific to this person's story and decision.
2. Classify the decision into exactly one of these categories: "Relocation & Lifestyle Shifts", "Housing & Major Purchases", "Career & Education Pivots", "Family, Relationships & Life Stage Changes", "Financial Milestones & Investments", "Health, Wellness & Personal Overhauls".
3. Score severity (0-10) and reversibility ("easily_reversible"|"reversible_with_cost"|"partially_reversible"|"irreversible").

Output JSON of shape:
{"questions": [{"question_text": "...", "sort_order": 1}], "category": "...", "severity": 5, "reversibility": "..."}"#;
    let user_ctx = user_context_summary(ctx);
    let decision_text = ctx
        .decision
        .as_ref()
        .map(|d| d.decision_text.as_str())
        .unwrap_or("");
    let user = format!(
        "User context:\n{}\n\nDecision under consideration: {}\n\nReturn JSON only.",
        user_ctx, decision_text
    );
    (sys.into(), user)
}

fn build_routine_inference(ctx: &SimulationContext) -> (String, String) {
    let sys = r#"You are an onboarding assistant. Infer the user's likely daily routine from their life story.

Output JSON: {"morning": ["activity 1", ...], "afternoon": ["..."], "night": ["..."]}
Each bucket should have 3-5 specific activities. Be concrete and personalized."#;
    let uc = user_context_summary(ctx);
    let user = format!("Based on this user context:\n{}\n\nReturn JSON only.", uc);
    (sys.into(), user)
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
    let uc = user_context_summary(ctx);
    let decision_text = ctx
        .decision
        .as_ref()
        .map(|d| d.decision_text.clone())
        .unwrap_or_default();
    let user = format!(
        "User context:\n{}\n\nDecision: {}\n\nTime horizon: {} months.\n\nReturn JSON only.",
        uc, decision_text, ctx.time_horizon_months
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

fn build_structured_synthesis(ctx: &SimulationContext) -> (String, String) {
    let sys = r#"You are an onboarding assistant. Given structured answers from the user, produce a concise AI summary and extracted_context.

Output JSON: {"ai_summary": "...", "extracted_context": {...}}"#;
    let qa = ctx
        .clarifying_qas
        .iter()
        .map(|q| format!("Q: {}\nA: {}", q.question_text, q.answer_text))
        .collect::<Vec<_>>()
        .join("\n\n");
    let user = format!("Answers:\n{}\n\nReturn JSON only.", qa);
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
