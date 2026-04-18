CREATE TABLE character_plates (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    source_photo_id UUID NOT NULL REFERENCES user_photos(id),
    storage_url TEXT NOT NULL DEFAULT '',
    storage_path TEXT NOT NULL DEFAULT '',
    prompt_used TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_character_plates_user_id ON character_plates(user_id);
CREATE INDEX idx_character_plates_status ON character_plates(status);
