CREATE TABLE practical_guides (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    simulation_id        UUID NOT NULL UNIQUE REFERENCES decision_simulations(id) ON DELETE CASCADE,
    actionable_steps     JSONB NOT NULL DEFAULT '[]',
    experiments          JSONB NOT NULL DEFAULT '[]',
    alternative_ideas    JSONB NOT NULL DEFAULT '[]',
    decision_checkpoints JSONB NOT NULL DEFAULT '[]',
    raw_ai_response      TEXT NOT NULL DEFAULT '',
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_practical_guides_simulation_id ON practical_guides(simulation_id);
