CREATE TABLE scenario_plans (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    simulation_id   UUID NOT NULL REFERENCES decision_simulations(id),
    path_a          JSONB NOT NULL DEFAULT '{}',
    path_b          JSONB NOT NULL DEFAULT '{}',
    shared_context  JSONB NOT NULL DEFAULT '{}',
    raw_ai_response TEXT NOT NULL DEFAULT '',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_scenario_plans_simulation ON scenario_plans(simulation_id);
