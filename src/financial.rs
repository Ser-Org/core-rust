//! Deterministic financial fact sheet + financial relevance classification.
//!
//! Ported from internal/orchestration/grounded_context.go and
//! internal/models/financial_relevance.go in the Go source. The output is
//! frozen into the simulation snapshot so downstream LLM jobs can cite
//! consistent numbers instead of inventing them.

use crate::models::{
    value_status, FinancialFactSheet, FinancialProfile, LifeState, TaggedValue, UserProfile,
};
use serde::{Deserialize, Serialize};

pub fn build_financial_fact_sheet(
    profile: &UserProfile,
    life_state: &LifeState,
    fp: &FinancialProfile,
) -> FinancialFactSheet {
    let mut sheet = FinancialFactSheet::default();

    // Yearly salary — exact from onboarding.
    if profile.estimated_yearly_salary > 0.0 {
        sheet.yearly_salary = TaggedValue {
            value: profile.estimated_yearly_salary,
            status: value_status::EXACT.into(),
            source: "yearly_salary".into(),
            ..TaggedValue::default()
        };
    } else {
        sheet.yearly_salary.status = value_status::UNKNOWN.into();
    }

    // Net worth — exact from onboarding.
    sheet.net_worth = TaggedValue {
        value: profile.estimated_net_worth,
        status: value_status::EXACT.into(),
        source: "net_worth".into(),
        ..TaggedValue::default()
    };

    // Monthly income — derived from yearly salary.
    if profile.estimated_yearly_salary > 0.0 {
        let monthly = profile.estimated_yearly_salary / 12.0;
        sheet.monthly_income = TaggedValue {
            value: round_money(monthly),
            status: value_status::DERIVED.into(),
            source: "yearly_salary/12".into(),
            note: "Monthly income = yearly salary / 12".into(),
            ..TaggedValue::default()
        };
    } else {
        sheet.monthly_income.status = value_status::UNKNOWN.into();
    }

    // Monthly spending — use user-provided value when available, else estimate.
    if life_state.monthly_spending > 0.0 {
        sheet.monthly_spending = TaggedValue {
            value: life_state.monthly_spending,
            status: value_status::EXACT.into(),
            source: "life_state.monthly_spending".into(),
            ..TaggedValue::default()
        };
    } else if sheet.monthly_income.value > 0.0 {
        // Rough default: 65-85% of income goes to spending. Use the midpoint as the
        // derived estimate but mark the bounds too.
        let midpoint = sheet.monthly_income.value * 0.75;
        sheet.monthly_spending = TaggedValue {
            value: round_money(midpoint),
            min: round_money(sheet.monthly_income.value * 0.65),
            max: round_money(sheet.monthly_income.value * 0.85),
            status: value_status::BOUNDED.into(),
            source: "monthly_income * 0.75 (0.65..0.85)".into(),
            note: "Estimated monthly spending as fraction of income".into(),
        };
    } else {
        sheet.monthly_spending.status = value_status::UNKNOWN.into();
    }

    // Monthly savings = income - spending, clamped to 0.
    if life_state.monthly_savings > 0.0 {
        sheet.monthly_savings = TaggedValue {
            value: life_state.monthly_savings,
            status: value_status::EXACT.into(),
            source: "life_state.monthly_savings".into(),
            ..TaggedValue::default()
        };
    } else if sheet.monthly_income.value > 0.0 && sheet.monthly_spending.value > 0.0 {
        let savings = (sheet.monthly_income.value - sheet.monthly_spending.value).max(0.0);
        sheet.monthly_savings = TaggedValue {
            value: round_money(savings),
            status: value_status::DERIVED.into(),
            source: "monthly_income - monthly_spending".into(),
            ..TaggedValue::default()
        };
    } else {
        sheet.monthly_savings.status = value_status::UNKNOWN.into();
    }

    // Housing cost.
    if life_state.housing_cost > 0.0 {
        sheet.housing_cost = TaggedValue {
            value: life_state.housing_cost,
            status: value_status::EXACT.into(),
            source: "life_state.housing_cost".into(),
            ..TaggedValue::default()
        };
    } else {
        sheet.housing_cost.status = value_status::UNKNOWN.into();
    }

    // Total debt.
    if life_state.debt > 0.0 {
        sheet.total_debt = TaggedValue {
            value: life_state.debt,
            status: value_status::EXACT.into(),
            source: "life_state.debt".into(),
            ..TaggedValue::default()
        };
    } else {
        sheet.total_debt.status = value_status::UNKNOWN.into();
    }

    // Liquid savings = net_worth * liquid fraction (clamped positive).
    let liquid_frac = if fp.liquid_net_worth_fraction > 0.0 {
        fp.liquid_net_worth_fraction
    } else {
        1.0
    };
    if profile.estimated_net_worth > 0.0 {
        let liquid = profile.estimated_net_worth * liquid_frac;
        let status = if liquid_frac == 1.0 {
            value_status::DERIVED
        } else {
            value_status::BOUNDED
        };
        sheet.liquid_savings = TaggedValue {
            value: round_money(liquid),
            status: status.into(),
            source: format!("net_worth * liquid_fraction ({:.2})", liquid_frac),
            ..TaggedValue::default()
        };
    } else {
        sheet.liquid_savings.status = value_status::UNKNOWN.into();
    }

    // Runway months = liquid_savings / monthly_spending.
    if sheet.liquid_savings.value > 0.0 && sheet.monthly_spending.value > 0.0 {
        let months = sheet.liquid_savings.value / sheet.monthly_spending.value;
        sheet.runway_months = TaggedValue {
            value: round_to(months, 1),
            status: value_status::DERIVED.into(),
            source: "liquid_savings / monthly_spending".into(),
            note: "How many months current liquid savings cover current spending".into(),
            ..TaggedValue::default()
        };
    } else {
        sheet.runway_months.status = value_status::UNKNOWN.into();
    }

    sheet
}

fn round_money(v: f64) -> f64 {
    round_to(v, 2)
}

fn round_to(v: f64, places: u32) -> f64 {
    let factor = 10f64.powi(places as i32);
    (v * factor).round() / factor
}

// --- Financial Relevance ----------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinancialRelevance {
    None,
    Conditional,
    Strong,
}

impl FinancialRelevance {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Conditional => "conditional",
            Self::Strong => "strong",
        }
    }
}

impl Default for FinancialRelevance {
    fn default() -> Self {
        Self::Conditional
    }
}

/// Classify how financially relevant a decision is based on its category.
///
/// Categories with inherent money impact (housing, financial milestones) get
/// "strong"; career/education pivots are "conditional"; relocation / lifestyle
/// shifts that may or may not touch finances default to "conditional"; health
/// and personal overhauls default to "none" unless other signals say otherwise.
pub fn decision_financial_relevance(category: &str) -> FinancialRelevance {
    match category {
        "Housing & Major Purchases" | "Financial Milestones & Investments" => {
            FinancialRelevance::Strong
        }
        "Career & Education Pivots" | "Relocation & Lifestyle Shifts" => {
            FinancialRelevance::Conditional
        }
        "Family, Relationships & Life Stage Changes" => FinancialRelevance::Conditional,
        "Health, Wellness & Personal Overhauls" => FinancialRelevance::None,
        _ => FinancialRelevance::Conditional,
    }
}

/// Returns true when the decision has no inherent financial dimension.
pub fn is_financially_neutral(relevance: FinancialRelevance) -> bool {
    matches!(relevance, FinancialRelevance::None)
}

/// True if the user's life context contains any financial signals that
/// downstream prompts should cite (income, net worth, debt, housing cost, etc).
pub fn has_financial_signals(profile: &UserProfile, life_state: &LifeState) -> bool {
    profile.estimated_net_worth != 0.0
        || profile.estimated_yearly_salary > 0.0
        || life_state.debt > 0.0
        || life_state.monthly_spending > 0.0
        || life_state.monthly_savings > 0.0
        || life_state.housing_cost > 0.0
}
