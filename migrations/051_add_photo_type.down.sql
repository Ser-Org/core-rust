-- 051_add_photo_type.down.sql

DROP INDEX IF EXISTS idx_user_photos_user_type_primary;

ALTER TABLE user_photos
    DROP COLUMN IF EXISTS photo_type;
