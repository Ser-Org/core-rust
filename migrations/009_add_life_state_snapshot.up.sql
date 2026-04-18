ALTER TABLE decision_simulations
ADD COLUMN life_state_snapshot JSONB;

COMMENT ON COLUMN decision_simulations.life_state_snapshot IS
'Typed LifeState JSON frozen at simulation dispatch time. Used by Monte Carlo engine.';
