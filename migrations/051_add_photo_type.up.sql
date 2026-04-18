-- 051_add_photo_type.up.sql
-- Adds a photo_type enum column to user_photos so users can have both a
-- "face" photo (head/shoulders — used by Runway character plate + Runway
-- video pipeline for close-up identity anchoring) and a "full_body" photo
-- (used by Flash/Flux for wide scenes where proportions matter).
--
-- The unique index is PARTIAL (WHERE is_primary = true) so it enforces
-- "at most one active face + one active body photo per user" without
-- conflicting with legacy inactive rows. Prior to this migration, older code
-- kept replaced photos around as is_primary=false rows so that
-- character_plates.source_photo_id foreign keys remained valid. Those rows
-- all backfill to photo_type='face' via the DEFAULT, and the partial index
-- ignores them, so the migration stays safe on databases with historical
-- re-uploads.

ALTER TABLE user_photos
    ADD COLUMN photo_type TEXT NOT NULL DEFAULT 'face'
        CHECK (photo_type IN ('face', 'full_body'));

CREATE UNIQUE INDEX idx_user_photos_user_type_primary
    ON user_photos (user_id, photo_type)
    WHERE is_primary = true;
