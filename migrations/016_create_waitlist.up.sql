-- 016_create_waitlist.up.sql
-- Creates the waitlist_entries table for pre-launch waitlist signups.

CREATE TABLE waitlist_entries (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email       TEXT NOT NULL,
    name        TEXT,
    ip_address  TEXT,
    user_agent  TEXT,
    source      TEXT NOT NULL DEFAULT 'landing',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Unique email constraint prevents duplicate signups.
CREATE UNIQUE INDEX idx_waitlist_entries_email ON waitlist_entries(LOWER(email));
CREATE INDEX idx_waitlist_entries_created_at ON waitlist_entries(created_at DESC);
