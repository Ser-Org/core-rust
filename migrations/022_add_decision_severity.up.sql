-- Add severity score and reversibility classification to decisions.
ALTER TABLE decisions
    ADD COLUMN IF NOT EXISTS severity      INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS reversibility TEXT    NOT NULL DEFAULT '';
