ALTER TABLE decision_simulations
ADD COLUMN confidence_score NUMERIC DEFAULT 0.0;

COMMENT ON COLUMN decision_simulations.confidence_score IS '0.0-1.0 score based on LifeState completeness at simulation dispatch time.';
