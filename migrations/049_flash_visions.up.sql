-- Flash visions: standalone "What if" experiences with cinematic image sequences.

CREATE TABLE flash_visions (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         UUID NOT NULL REFERENCES users(id),
    question        TEXT NOT NULL,
    input_method    TEXT NOT NULL DEFAULT 'text',
    status          TEXT NOT NULL DEFAULT 'pending',
    photo_url       TEXT,
    music_url       TEXT,
    error_message   TEXT,
    share_token     TEXT UNIQUE,
    is_public       BOOLEAN DEFAULT false,
    completed_at    TIMESTAMPTZ,
    created_at      TIMESTAMPTZ DEFAULT now(),
    updated_at      TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX idx_flash_visions_user_id ON flash_visions(user_id);
CREATE INDEX idx_flash_visions_status ON flash_visions(user_id, status);
CREATE INDEX idx_flash_visions_share_token ON flash_visions(share_token) WHERE share_token IS NOT NULL;

CREATE TABLE flash_images (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    flash_vision_id     UUID NOT NULL REFERENCES flash_visions(id),
    index               INT NOT NULL,
    storage_url         TEXT NOT NULL,
    storage_path        TEXT NOT NULL,
    prompt_used         TEXT NOT NULL,
    style_reference_id  UUID REFERENCES flash_images(id),
    generation_metadata JSONB,
    created_at          TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX idx_flash_images_vision_id ON flash_images(flash_vision_id);
CREATE INDEX idx_flash_images_vision_order ON flash_images(flash_vision_id, index);

-- Add Flash billing columns to subscriptions.
ALTER TABLE subscriptions
    ADD COLUMN flash_used  INT NOT NULL DEFAULT 0,
    ADD COLUMN flash_limit INT NOT NULL DEFAULT 1;
