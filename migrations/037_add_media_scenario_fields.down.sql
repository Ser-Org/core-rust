DROP INDEX IF EXISTS idx_generated_media_scenario;

ALTER TABLE generated_media
    DROP COLUMN IF EXISTS scenario_path,
    DROP COLUMN IF EXISTS scenario_phase;
