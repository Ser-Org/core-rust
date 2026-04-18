-- 006_create_media.up.sql
-- Generated media (images and videos) linked to simulations.

CREATE TABLE generated_media (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    simulation_id     UUID NOT NULL REFERENCES decision_simulations(id) ON DELETE CASCADE,
    media_type        TEXT NOT NULL CHECK (media_type IN ('image', 'video')),
    storage_url       TEXT NOT NULL DEFAULT '',
    storage_path      TEXT NOT NULL DEFAULT '',
    prompt_used       TEXT NOT NULL DEFAULT '',
    provider_metadata JSONB NOT NULL DEFAULT '{}',
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_generated_media_simulation_id ON generated_media(simulation_id);
CREATE INDEX idx_generated_media_type ON generated_media(simulation_id, media_type);
