-- 005_create_simulations.up.sql
-- Decision simulation coordination and all result tables.

CREATE TABLE decision_simulations (
    id                    UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    decision_id           UUID NOT NULL UNIQUE REFERENCES decisions(id) ON DELETE CASCADE,
    user_id               UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    status                TEXT NOT NULL DEFAULT 'running' CHECK (status IN ('running', 'completed', 'completed_partial', 'failed')),
    total_components      INT NOT NULL DEFAULT 5,
    completed_components  INT NOT NULL DEFAULT 0,
    user_context_snapshot JSONB NOT NULL DEFAULT '{}',
    started_at            TIMESTAMPTZ,
    completed_at          TIMESTAMPTZ,
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_decision_simulations_decision_id ON decision_simulations(decision_id);
CREATE INDEX idx_decision_simulations_status ON decision_simulations(status);

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
    created_at               TIMESTAMPTZ NOT NULL DEFAULT now()
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
    full_narrative  TEXT NOT NULL DEFAULT '',
    raw_ai_response TEXT NOT NULL DEFAULT '',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_narrative_arcs_simulation_id ON narrative_arcs(simulation_id);
