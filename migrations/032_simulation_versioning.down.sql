ALTER TABLE decision_simulations
  DROP COLUMN IF EXISTS parent_simulation_id,
  DROP COLUMN IF EXISTS run_number,
  DROP COLUMN IF EXISTS assumption_overrides;
