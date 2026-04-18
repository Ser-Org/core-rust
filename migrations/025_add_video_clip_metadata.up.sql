ALTER TABLE generated_media
    ADD COLUMN clip_role  TEXT NOT NULL DEFAULT 'hero',
    ADD COLUMN clip_order INTEGER NOT NULL DEFAULT 0;

CREATE INDEX idx_generated_media_clip_role
    ON generated_media(simulation_id, media_type, clip_role);
