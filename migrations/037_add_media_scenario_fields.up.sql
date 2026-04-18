ALTER TABLE generated_media
    ADD COLUMN IF NOT EXISTS scenario_path  TEXT,
    ADD COLUMN IF NOT EXISTS scenario_phase INTEGER;

CREATE INDEX idx_generated_media_scenario ON generated_media(simulation_id, scenario_path, scenario_phase);
