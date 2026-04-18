ALTER TABLE decisions
    DROP COLUMN IF EXISTS reversibility,
    DROP COLUMN IF EXISTS severity;
