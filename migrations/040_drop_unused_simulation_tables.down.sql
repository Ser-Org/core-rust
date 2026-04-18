-- Recreate tables with their final evolved schemas
-- (005 + 010 for financial_projections, 005 for probability_outcomes,
-- 005 + 023 for narrative_arcs, 026 + 027 for practical_guides).

CREATE TABLE financial_projections (
    id                       UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    simulation_id            UUID NOT NULL UNIQUE REFERENCES decision_simulations(id) ON DELETE CASCADE,
    income_curve             JSONB NOT NULL DEFAULT '[]',
    net_worth_best           JSONB NOT NULL DEFAULT '[]',
    net_worth_likely         JSONB NOT NULL DEFAULT '[]',
    net_worth_worst          JSONB NOT NULL DEFAULT '[]',
    risk_points              JSONB NOT NULL DEFAULT '[]',
    inflection_points        JSONB NOT NULL DEFAULT '[]',
    non_obvious_consequences JSONB NOT NULL DEFAULT '[]',
    raw_ai_response          TEXT NOT NULL DEFAULT '',
    created_at               TIMESTAMPTZ NOT NULL DEFAULT now(),
    mc_assumptions           JSONB,
    mc_aggregated            JSONB
);
CREATE INDEX idx_financial_projections_simulation_id ON financial_projections(simulation_id);

CREATE TABLE probability_outcomes (
    id                      UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    simulation_id           UUID NOT NULL UNIQUE REFERENCES decision_simulations(id) ON DELETE CASCADE,
    best_case_summary       TEXT NOT NULL DEFAULT '',
    best_case_probability   NUMERIC NOT NULL DEFAULT 0,
    likely_case_summary     TEXT NOT NULL DEFAULT '',
    likely_case_probability NUMERIC NOT NULL DEFAULT 0,
    worst_case_summary      TEXT NOT NULL DEFAULT '',
    worst_case_probability  NUMERIC NOT NULL DEFAULT 0,
    key_risks               JSONB NOT NULL DEFAULT '[]',
    critical_assumptions    JSONB NOT NULL DEFAULT '[]',
    raw_ai_response         TEXT NOT NULL DEFAULT '',
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_probability_outcomes_simulation_id ON probability_outcomes(simulation_id);

CREATE TABLE narrative_arcs (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    simulation_id   UUID NOT NULL UNIQUE REFERENCES decision_simulations(id) ON DELETE CASCADE,
    dispatches      JSONB NOT NULL DEFAULT '[]',
    branch_points   JSONB NOT NULL DEFAULT '[]',
    full_narrative  TEXT NOT NULL DEFAULT '',
    raw_ai_response TEXT NOT NULL DEFAULT '',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_narrative_arcs_simulation_id ON narrative_arcs(simulation_id);

CREATE TABLE practical_guides (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    simulation_id        UUID NOT NULL UNIQUE REFERENCES decision_simulations(id) ON DELETE CASCADE,
    actionable_steps     JSONB NOT NULL DEFAULT '[]',
    experiments          JSONB NOT NULL DEFAULT '[]',
    alternative_ideas    JSONB NOT NULL DEFAULT '[]',
    decision_checkpoints JSONB NOT NULL DEFAULT '[]',
    pivot_metrics        JSONB NOT NULL DEFAULT '{}',
    raw_ai_response      TEXT NOT NULL DEFAULT '',
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_practical_guides_simulation_id ON practical_guides(simulation_id);
