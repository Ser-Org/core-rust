//! A minimal life-causal graph used for prioritizing clarifying questions.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Node {
    pub id: String,
    pub label: String,
    pub category: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub weight: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CausalGraph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

impl CausalGraph {
    pub fn default_life_graph() -> Self {
        let nodes = vec![
            node("age", "Age", "demographic"),
            node("income", "Income", "financial"),
            node("net_worth", "Net Worth", "financial"),
            node("debt", "Debt", "financial"),
            node("career_stage", "Career Stage", "career"),
            node("industry", "Industry", "career"),
            node("role", "Role", "career"),
            node("health", "Health", "health"),
            node("relationship", "Relationship Status", "relationship"),
            node("dependents", "Dependents", "relationship"),
            node("risk_tolerance", "Risk Tolerance", "behavioral"),
            node("optimism", "Optimism", "behavioral"),
            node("stress_response", "Stress Response", "behavioral"),
            node("decision_style", "Decision Style", "behavioral"),
            node("location", "Location", "geographic"),
            node("geographic_mobility", "Geographic Mobility", "geographic"),
            node("goals", "Goals", "motivation"),
            node("saving_habits", "Saving Habits", "financial"),
            node("housing_cost", "Housing Cost", "financial"),
        ];
        let edges = vec![
            edge("career_stage", "income", 0.8),
            edge("industry", "income", 0.6),
            edge("role", "income", 0.6),
            edge("income", "net_worth", 0.7),
            edge("debt", "net_worth", 0.8),
            edge("health", "income", 0.3),
            edge("relationship", "dependents", 0.6),
            edge("dependents", "saving_habits", 0.4),
            edge("risk_tolerance", "saving_habits", 0.5),
            edge("location", "housing_cost", 0.7),
            edge("geographic_mobility", "career_stage", 0.4),
            edge("goals", "career_stage", 0.5),
            edge("age", "career_stage", 0.5),
            edge("age", "health", 0.3),
            edge("age", "relationship", 0.3),
            edge("stress_response", "decision_style", 0.5),
            edge("optimism", "risk_tolerance", 0.4),
            edge("industry", "career_stage", 0.4),
        ];
        Self { nodes, edges }
    }
}

fn node(id: &str, label: &str, category: &str) -> Node {
    Node {
        id: id.into(),
        label: label.into(),
        category: category.into(),
    }
}

fn edge(from: &str, to: &str, w: f64) -> Edge {
    Edge {
        from: from.into(),
        to: to.into(),
        weight: w,
    }
}
