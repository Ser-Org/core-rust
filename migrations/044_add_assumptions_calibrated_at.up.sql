ALTER TABLE decision_simulations
  ADD COLUMN assumptions_calibrated_at TIMESTAMPTZ;

COMMENT ON COLUMN decision_simulations.assumptions_calibrated_at IS
  'Timestamp when Teach Scout was used for this simulation; assumption editing/calibration is locked after this point.';
