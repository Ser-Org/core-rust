-- Phase 1: Promote assumptions and risks into first-class entities.
-- These were previously embedded in probability_outcomes.critical_assumptions
-- and probability_outcomes.key_risks JSONB arrays.

CREATE TABLE simulation_assumptions (
  id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  simulation_id   UUID NOT NULL REFERENCES decision_simulations(id) ON DELETE CASCADE,
  description     TEXT NOT NULL,
  confidence      NUMERIC(3,2) NOT NULL,       -- 0.00-1.00
  source          TEXT,                         -- provenance (user_input, life_state, reference_data, inferred)
  category        TEXT NOT NULL,                -- financial, behavioral, environmental, social, health, career, lifestyle
  editable        BOOLEAN NOT NULL DEFAULT true,
  user_override_value TEXT,                     -- null until user edits; stores edited description
  original_confidence NUMERIC(3,2),             -- preserved when user edits confidence
  created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_sim_assumptions_sim_id ON simulation_assumptions(simulation_id);

CREATE TABLE simulation_risks (
  id                    UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  simulation_id         UUID NOT NULL REFERENCES decision_simulations(id) ON DELETE CASCADE,
  description           TEXT NOT NULL,
  likelihood            TEXT NOT NULL,           -- low, medium, high
  impact                TEXT NOT NULL,           -- low, medium, high
  category              TEXT,                    -- financial, health, career, relationship, lifestyle
  linked_assumption_ids UUID[],                  -- references simulation_assumptions.id
  mitigation_hint       TEXT,
  created_at            TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_sim_risks_sim_id ON simulation_risks(simulation_id);
