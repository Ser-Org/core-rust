DROP INDEX IF EXISTS idx_generated_media_clip_role;

ALTER TABLE generated_media
    DROP COLUMN IF EXISTS clip_role,
    DROP COLUMN IF EXISTS clip_order;
