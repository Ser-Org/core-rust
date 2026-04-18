CREATE TABLE IF NOT EXISTS decision_briefs (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    simulation_id   UUID NOT NULL REFERENCES decision_simulations(id) ON DELETE CASCADE,
    recommendation  JSONB NOT NULL DEFAULT '{}',
    tradeoff_map    JSONB NOT NULL DEFAULT '{}',
    hidden_risks    JSONB NOT NULL DEFAULT '{}',
    next_actions    JSONB NOT NULL DEFAULT '{}',
    checkpoints     JSONB NOT NULL DEFAULT '{}',
    raw_ai_response TEXT NOT NULL DEFAULT '',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_decision_briefs_simulation_id ON decision_briefs(simulation_id);
