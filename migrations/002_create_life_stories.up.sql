-- 002_create_life_stories.up.sql
-- Stores the user's life story narrative and AI-extracted structured context.

CREATE TABLE life_stories (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id           UUID NOT NULL UNIQUE REFERENCES users(id) ON DELETE CASCADE,
    raw_input         TEXT NOT NULL DEFAULT '',
    input_method      TEXT NOT NULL DEFAULT 'text',
    ai_summary        TEXT NOT NULL DEFAULT '',
    extracted_context JSONB NOT NULL DEFAULT '{}',
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_life_stories_user_id ON life_stories(user_id);
