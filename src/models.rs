//! Domain models — direct ports of scout-core/internal/models/*.go.
//!
//! All structs use serde with the same JSON field names as the Go code so the
//! frontend sees byte-identical responses.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::FromRow;
use uuid::Uuid;

// --- Users & Onboarding ---

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub mod onboarding_status {
    pub const STORY_SUBMITTED: &str = "story_submitted";
    pub const BEHAVIORAL_PROFILE: &str = "behavioral_profile";
    pub const LIFE_CONTEXT_COMPLETED: &str = "life_context_completed";
    pub const PROFILE_COMPLETED: &str = "profile_completed";
    pub const NET_WORTH_SOURCE_COMPLETED: &str = "net_worth_source_completed";
    pub const COMPLETE: &str = "complete";
}

pub mod onboarding_path {
    pub const STORY: &str = "story";
    pub const QUESTIONS: &str = "questions";
}

pub mod input_method {
    pub const TEXT: &str = "text";
    pub const VOICE: &str = "voice";
}

pub mod age_bracket {
    pub const B18_24: &str = "18-24";
    pub const B25_34: &str = "25-34";
    pub const B35_44: &str = "35-44";
    pub const B45_54: &str = "45-54";
    pub const B55_64: &str = "55-64";
    pub const B65_PLUS: &str = "65+";
    pub fn is_valid(s: &str) -> bool {
        matches!(s, "18-24" | "25-34" | "35-44" | "45-54" | "55-64" | "65+")
    }
}

pub mod gender {
    pub fn is_valid(s: &str) -> bool {
        matches!(s, "male" | "female" | "non_binary" | "prefer_not_to_say")
    }
}

pub mod living_situation {
    pub fn is_valid(s: &str) -> bool {
        matches!(s, "rent" | "own" | "nomadic" | "with_family")
    }
}

pub mod net_worth_bracket {
    pub fn is_valid(s: &str) -> bool {
        matches!(s, "starting_out" | "building" | "established" | "wealthy")
    }
}

pub mod income_bracket {
    pub fn is_valid(s: &str) -> bool {
        matches!(s, "modest" | "comfortable" | "high" | "top_tier")
    }
}

pub mod career_stage {
    pub fn is_valid(s: &str) -> bool {
        matches!(
            s,
            "starting" | "building" | "established" | "senior" | "between_chapters"
        )
    }
}

pub mod industry {
    pub fn is_valid(s: &str) -> bool {
        matches!(
            s,
            "office_tech"
                | "creative_media"
                | "healthcare"
                | "trades_physical"
                | "hospitality"
                | "education"
                | "finance_law"
                | "other"
        )
    }
}

pub mod photo_type {
    pub const FACE: &str = "face";
    pub const FULL_BODY: &str = "full_body";
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct UserProfile {
    pub id: Uuid,
    pub user_id: Uuid,
    pub estimated_net_worth: f64,
    pub estimated_yearly_salary: f64,
    pub onboarding_status: String,
    pub risk_tolerance: Option<String>,
    pub follow_through: Option<String>,
    pub optimism_bias: Option<String>,
    pub stress_response: Option<String>,
    pub decision_style: Option<String>,
    pub saving_habits: Option<String>,
    pub debt_comfort: Option<String>,
    pub housing_stability: Option<String>,
    pub income_stability: Option<String>,
    pub liquid_net_worth_source: Option<String>,
    pub relationship_status: Option<String>,
    pub household_income_structure: Option<String>,
    pub dependent_count: Option<i32>,
    pub life_stability: Option<String>,
    pub onboarding_path: String,
    pub age_bracket: Option<String>,
    pub gender: Option<String>,
    pub living_situation: Option<String>,
    pub industry: Option<String>,
    pub career_stage: Option<String>,
    pub net_worth_bracket: Option<String>,
    pub income_bracket: Option<String>,
    pub cinematic_context_completed: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BehavioralProfile {
    pub risk_tolerance: String,
    pub follow_through: String,
    pub optimism_bias: String,
    pub stress_response: String,
    pub decision_style: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FinancialProfile {
    pub saving_habits: String,
    pub debt_comfort: String,
    pub housing_stability: String,
    pub income_stability: String,
    pub liquid_net_worth_fraction: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LifeContextProfile {
    pub relationship_status: String,
    pub household_income_structure: String,
    pub dependent_count: i32,
    pub life_stability: String,
}

pub fn resolve_behavioral_profile(p: &UserProfile) -> BehavioralProfile {
    let r = |v: &Option<String>, d: &str| {
        v.as_deref().filter(|s| !s.is_empty()).unwrap_or(d).to_string()
    };
    BehavioralProfile {
        risk_tolerance: r(&p.risk_tolerance, "moderate"),
        follow_through: r(&p.follow_through, "moderate"),
        optimism_bias: r(&p.optimism_bias, "moderate"),
        stress_response: r(&p.stress_response, "analytical"),
        decision_style: r(&p.decision_style, "deliberate"),
    }
}

pub fn resolve_financial_profile(p: &UserProfile) -> FinancialProfile {
    let r = |v: &Option<String>, d: &str| {
        v.as_deref().filter(|s| !s.is_empty()).unwrap_or(d).to_string()
    };
    let liquid = p.liquid_net_worth_source.as_deref().unwrap_or("");
    let frac = match liquid {
        "illiquid" => 0.15,
        "mostly_illiquid" => 0.30,
        "mixed" => 0.55,
        "liquid" => 0.85,
        _ => 1.0,
    };
    FinancialProfile {
        saving_habits: r(&p.saving_habits, ""),
        debt_comfort: r(&p.debt_comfort, ""),
        housing_stability: r(&p.housing_stability, ""),
        income_stability: r(&p.income_stability, ""),
        liquid_net_worth_fraction: frac,
    }
}

pub fn resolve_life_context_profile(p: &UserProfile) -> LifeContextProfile {
    LifeContextProfile {
        relationship_status: p
            .relationship_status
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_default(),
        household_income_structure: p
            .household_income_structure
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_default(),
        dependent_count: p.dependent_count.unwrap_or(0),
        life_stability: p
            .life_stability
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_default(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct LifeStory {
    pub id: Uuid,
    pub user_id: Uuid,
    pub raw_input: String,
    pub input_method: String,
    pub ai_summary: String,
    pub extracted_context: JsonValue,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct UserPhoto {
    pub id: Uuid,
    pub user_id: Uuid,
    pub storage_url: String,
    pub storage_path: String,
    pub mime_type: String,
    pub is_primary: bool,
    pub photo_type: String,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flux_storage_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flux_storage_path: Option<String>,
}

pub mod character_plate_status {
    pub const PENDING: &str = "pending";
    pub const GENERATING: &str = "generating";
    pub const READY: &str = "ready";
    pub const FAILED: &str = "failed";
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct CharacterPlate {
    pub id: Uuid,
    pub user_id: Uuid,
    pub source_photo_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_bucket: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    pub prompt_used: String,
    pub status: String,
    pub attempt_count: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// --- Decisions ---

pub mod decision_status {
    pub const DRAFT: &str = "draft";
    pub const SIMULATING: &str = "simulating";
    pub const COMPLETED: &str = "completed";
    pub const FAILED: &str = "failed";
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Decision {
    pub id: Uuid,
    pub user_id: Uuid,
    pub decision_text: String,
    pub input_method: String,
    pub time_horizon_months: i32,
    pub status: String,
    pub category: String,
    pub severity: i32,
    pub reversibility: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub share_token: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// --- Simulations ---

pub mod simulation_status {
    pub const RUNNING: &str = "running";
    pub const COMPLETED: &str = "completed";
    pub const COMPLETED_PARTIAL: &str = "completed_partial";
    pub const FAILED: &str = "failed";
}

pub mod simulation_run_type {
    pub const CINEMATIC: &str = "cinematic";
    pub const TEXT_ONLY: &str = "text_only";
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct DecisionSimulation {
    pub id: Uuid,
    pub decision_id: Uuid,
    pub user_id: Uuid,
    pub status: String,
    pub total_components: i32,
    pub completed_components: i32,
    pub run_type: String,
    pub user_context_snapshot: JsonValue,
    pub life_state_snapshot: JsonValue,
    pub data_completeness: f64,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_simulation_id: Option<Uuid>,
    pub run_number: i32,
    #[serde(skip_serializing_if = "is_null_json")]
    pub assumption_overrides: JsonValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assumptions_calibrated_at: Option<DateTime<Utc>>,
}

fn is_null_json(v: &JsonValue) -> bool {
    v.is_null()
}

// NOTE: Legacy per-simulation textual outputs (ProbabilityOutcomes,
// FinancialProjections, DecisionBrief, NarrativeArc) were removed from the
// live pipeline in upstream commit d3860ae. The tables still exist for
// historical rows but nothing writes to them. Video prompts pull their
// context from the scenario plan phases + assumption extraction outputs.

// --- Simulation Components ---

pub mod simulation_component_status {
    pub const PENDING: &str = "pending";
    pub const RUNNING: &str = "running";
    pub const COMPLETED: &str = "completed";
    pub const FAILED: &str = "failed";
}

pub mod simulation_component_type {
    pub const SCENARIO_PLAN: &str = "scenario_plan";
    pub const ASSUMPTIONS: &str = "assumptions";
    pub const CHARACTER_PLATE: &str = "character_palette";
    pub const VIDEO_CLIP: &str = "video_clip";
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SimulationComponent {
    pub id: Uuid,
    pub simulation_id: Uuid,
    pub component_key: String,
    pub component_type: String,
    pub display_name: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    pub metadata: JsonValue,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

// --- Scenario Plans ---

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ScenarioPlan {
    pub id: Uuid,
    pub simulation_id: Uuid,
    pub path_a: JsonValue,
    pub path_b: JsonValue,
    pub shared_context: JsonValue,
    pub raw_ai_response: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioPath {
    pub label: String,
    pub outcome: String,
    pub phases: Vec<ScenarioPhase>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioPhase {
    pub index: i32,
    pub title: String,
    pub time_label: String,
    pub scene_prompt: String,
    pub motion_prompt: String,
    pub edit_prompt: String,
    pub narration_text: String,
    pub overlay_data: PhaseOverlayData,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PhaseOverlayData {
    pub primary_metric: MetricPoint,
    pub secondary_metric: MetricPoint,
    pub mood_tag: String,
    pub risk_level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MetricPoint {
    pub label: String,
    pub value: String,
    pub trend: String,
}

// --- Media ---

pub mod media_type {
    pub const IMAGE: &str = "image";
    pub const VIDEO: &str = "video";
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct GeneratedMedia {
    pub id: Uuid,
    pub simulation_id: Uuid,
    pub media_type: String,
    pub storage_url: String,
    pub storage_path: String,
    pub prompt_used: String,
    pub provider_metadata: JsonValue,
    pub clip_role: String,
    pub clip_order: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scenario_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scenario_phase: Option<i32>,
    pub created_at: DateTime<Utc>,
}

// --- Subscriptions / Billing ---

pub mod plan_type {
    pub const FREE: &str = "free";
    pub const EXPLORER: &str = "explorer";
    pub const PRO: &str = "pro";
    pub const UNLIMITED: &str = "unlimited";
    pub const STARTER: &str = "starter";
    pub const FAMILY: &str = "family";

    pub fn simulation_limit(plan: &str) -> i32 {
        match plan {
            FREE => 1,
            EXPLORER | STARTER => 3,
            PRO => 10,
            UNLIMITED | FAMILY => 30,
            _ => 0,
        }
    }

    pub fn flash_limit(plan: &str) -> i32 {
        match plan {
            FREE => 2,
            EXPLORER | STARTER => 10,
            PRO => 30,
            UNLIMITED | FAMILY => 75,
            _ => 0,
        }
    }
}

pub const EXTRA_SIMULATION_PRICE_CENTS: i32 = 1500;

pub mod entitlement_code {
    pub const BILLING_INACTIVE: &str = "billing_inactive";
    pub const CINEMATIC_LIMIT_REACHED: &str = "cinematic_limit_reached";
    pub const FLASH_LIMIT_REACHED: &str = "flash_limit_reached";
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitlementError {
    pub code: String,
    pub message: String,
}

pub mod subscription_status {
    pub const ACTIVE: &str = "active";
    pub const TRIALING: &str = "trialing";
    pub const PAST_DUE: &str = "past_due";
    pub const CANCELED: &str = "canceled";
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Subscription {
    pub id: Uuid,
    pub user_id: Uuid,
    pub stripe_customer_id: Option<String>,
    pub stripe_subscription_id: Option<String>,
    pub plan: String,
    pub status: String,
    pub cinematic_used: i32,
    pub cinematic_limit: i32,
    pub text_resim_used: i32,
    pub text_resim_limit: i32,
    pub extra_cinematic_credits: i32,
    pub flash_used: i32,
    pub flash_limit: i32,
    pub period_start: Option<DateTime<Utc>>,
    pub period_end: Option<DateTime<Utc>>,
    pub cancel_at_period_end: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Subscription {
    pub fn billing_active(&self) -> bool {
        self.status == subscription_status::ACTIVE
            || self.status == subscription_status::TRIALING
    }
    pub fn remaining_simulations(&self) -> i32 {
        (self.cinematic_limit - self.cinematic_used).max(0)
    }
}

// --- LifeState ---

pub mod score_level {
    pub const LOW: &str = "low";
    pub const MEDIUM: &str = "medium";
    pub const HIGH: &str = "high";
    pub const UNKNOWN: &str = "unknown";
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LifeState {
    #[serde(default)]
    pub age: i32,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub age_range: String,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub age_source: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub gender: String,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub gender_source: String,
    #[serde(default)]
    pub education_level: String,
    #[serde(default)]
    pub industry: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub profession: String,
    #[serde(default)]
    pub career_experience_yr: i32,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub career_stage: String,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub career_experience_source: String,
    #[serde(default)]
    pub income: f64,
    #[serde(default)]
    pub net_worth: f64,
    #[serde(default)]
    pub debt: f64,
    #[serde(skip_serializing_if = "is_zero_f64", default)]
    pub monthly_spending: f64,
    #[serde(skip_serializing_if = "is_zero_f64", default)]
    pub monthly_savings: f64,
    #[serde(skip_serializing_if = "is_zero_f64", default)]
    pub housing_cost: f64,
    pub risk_tolerance: String,
    #[serde(default)]
    pub health_score: f64,
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub health_provided: bool,
    pub network_strength: String,
    pub ambition: String,
    pub stress_level: String,
    pub relationship_status: String,
    pub has_dependents: Option<bool>,
    #[serde(default)]
    pub dependent_count: i32,
    #[serde(default)]
    pub goals: Vec<String>,
    pub geographic_mobility: String,
    #[serde(default)]
    pub known_fields: i32,
    #[serde(default)]
    pub total_fields: i32,
    #[serde(default)]
    pub completeness: f64,
}

fn is_zero_f64(v: &f64) -> bool {
    *v == 0.0
}

impl LifeState {
    pub fn default_state() -> Self {
        Self {
            risk_tolerance: score_level::UNKNOWN.into(),
            health_score: 0.75,
            network_strength: score_level::UNKNOWN.into(),
            ambition: score_level::UNKNOWN.into(),
            stress_level: score_level::UNKNOWN.into(),
            relationship_status: "unknown".into(),
            geographic_mobility: score_level::UNKNOWN.into(),
            total_fields: 22,
            ..Default::default()
        }
    }

    pub fn compute_completeness(&mut self) {
        self.total_fields = 22;
        let mut known = 0;
        if self.age > 0 {
            known += 1;
        }
        if !self.location.is_empty() {
            known += 1;
        }
        if !self.education_level.is_empty() {
            known += 1;
        }
        if !self.industry.is_empty() {
            known += 1;
        }
        if !self.role.is_empty() {
            known += 1;
        }
        if !self.profession.is_empty() {
            known += 1;
        }
        if self.career_experience_yr > 0 {
            known += 1;
        }
        if self.income > 0.0 {
            known += 1;
        }
        if self.net_worth != 0.0 {
            known += 1;
        }
        if self.risk_tolerance != score_level::UNKNOWN && !self.risk_tolerance.is_empty() {
            known += 1;
        }
        if self.health_provided {
            known += 1;
        }
        if self.network_strength != score_level::UNKNOWN && !self.network_strength.is_empty() {
            known += 1;
        }
        if self.ambition != score_level::UNKNOWN && !self.ambition.is_empty() {
            known += 1;
        }
        if self.relationship_status != "unknown" && !self.relationship_status.is_empty() {
            known += 1;
        }
        if self.has_dependents.is_some() {
            known += 1;
        }
        if self.geographic_mobility != score_level::UNKNOWN && !self.geographic_mobility.is_empty()
        {
            known += 1;
        }
        if !self.goals.is_empty() {
            known += 1;
        }
        if self.stress_level != score_level::UNKNOWN && !self.stress_level.is_empty() {
            known += 1;
        }
        if self.debt > 0.0 {
            known += 1;
        }
        if self.monthly_spending > 0.0 {
            known += 1;
        }
        if self.monthly_savings > 0.0 {
            known += 1;
        }
        if self.housing_cost > 0.0 {
            known += 1;
        }
        self.known_fields = known;
        if self.total_fields > 0 {
            self.completeness = known as f64 / self.total_fields as f64;
        }
    }
}

// --- Flash ---

pub mod flash_status {
    pub const PENDING: &str = "pending";
    pub const GENERATING: &str = "generating";
    pub const COMPLETED: &str = "completed";
    pub const FAILED: &str = "failed";
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FlashVision {
    pub id: Uuid,
    pub user_id: Uuid,
    pub question: String,
    pub input_method: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub photo_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub music_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub share_token: Option<String>,
    pub is_public: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FlashImage {
    pub id: Uuid,
    pub flash_vision_id: Uuid,
    pub index: i32,
    pub storage_url: String,
    pub storage_path: String,
    pub prompt_used: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style_reference_id: Option<Uuid>,
    #[serde(skip_serializing_if = "JsonValue::is_null")]
    pub generation_metadata: JsonValue,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashPromptPlan {
    pub scenario_title: String,
    pub mood: String,
    pub music_mood: String,
    pub style_anchor: String,
    pub prompts: Vec<FlashScenePrompt>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashScenePrompt {
    pub index: i32,
    pub scene: String,
    pub prompt: String,
}

// --- Assumptions / Risks ---

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SimulationAssumption {
    pub id: Uuid,
    pub simulation_id: Uuid,
    pub description: String,
    pub confidence: f64,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub source: String,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub kind: String,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub grounding: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub evidence_refs: Vec<String>,
    pub category: String,
    pub editable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_override_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_confidence: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_field: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SimulationRisk {
    pub id: Uuid,
    pub simulation_id: Uuid,
    pub description: String,
    pub likelihood: String,
    pub impact: String,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub category: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub linked_assumption_ids: Vec<Uuid>,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub mitigation_hint: String,
    pub created_at: DateTime<Utc>,
}

// --- Waitlist ---

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WaitlistEntry {
    pub id: Uuid,
    pub email: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip)]
    pub ip_address: Option<String>,
    #[serde(skip)]
    pub user_agent: Option<String>,
    pub source: String,
    pub created_at: DateTime<Utc>,
}

// --- Financial Fact Sheet ---

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaggedValue {
    #[serde(skip_serializing_if = "is_zero_f64", default)]
    pub value: f64,
    #[serde(skip_serializing_if = "is_zero_f64", default)]
    pub min: f64,
    #[serde(skip_serializing_if = "is_zero_f64", default)]
    pub max: f64,
    pub status: String,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub source: String,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub note: String,
}

pub mod value_status {
    pub const EXACT: &str = "exact";
    pub const DERIVED: &str = "derived";
    pub const BOUNDED: &str = "bounded";
    pub const UNKNOWN: &str = "unknown";
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FinancialFactSheet {
    pub monthly_income: TaggedValue,
    pub monthly_spending: TaggedValue,
    pub monthly_savings: TaggedValue,
    pub housing_cost: TaggedValue,
    pub liquid_savings: TaggedValue,
    pub total_debt: TaggedValue,
    pub runway_months: TaggedValue,
    pub net_worth: TaggedValue,
    pub yearly_salary: TaggedValue,
}

// --- Gate Error ---

pub mod gate_code {
    pub const CINEMATIC_CONTEXT_REQUIRED: &str = "CINEMATIC_CONTEXT_REQUIRED";
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateError {
    pub code: String,
    pub message: String,
}

// --- Reference ---

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ReferenceDocument {
    pub id: Uuid,
    pub storage_path: String,
    pub bucket: String,
    pub title: String,
    pub category: String,
    pub content_type: String,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_refreshed_at: Option<DateTime<Utc>>,
    pub active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
