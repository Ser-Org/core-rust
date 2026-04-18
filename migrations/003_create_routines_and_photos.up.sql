-- 003_create_routines_and_photos.up.sql
-- Daily routine activities and user photo references.

CREATE TABLE routines (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    period     TEXT NOT NULL CHECK (period IN ('morning', 'afternoon', 'night')),
    activity   TEXT NOT NULL,
    confirmed  BOOLEAN NOT NULL DEFAULT false,
    sort_order INT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_routines_user_id ON routines(user_id);

CREATE TABLE user_photos (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id      UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    storage_url  TEXT NOT NULL,
    storage_path TEXT NOT NULL,
    is_primary   BOOLEAN NOT NULL DEFAULT false,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_user_photos_user_id ON user_photos(user_id);
