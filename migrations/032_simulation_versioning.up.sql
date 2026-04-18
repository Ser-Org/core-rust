-- Phase 6: Targeted re-simulation — track simulation lineage and assumption overrides.

ALTER TABLE decision_simulations
  ADD COLUMN parent_simulation_id UUID REFERENCES decision_simulations(id),
  ADD COLUMN run_number INT NOT NULL DEFAULT 1,
  ADD COLUMN assumption_overrides JSONB;

CREATE INDEX idx_sim_parent ON decision_simulations(parent_simulation_id) WHERE parent_simulation_id IS NOT NULL;
