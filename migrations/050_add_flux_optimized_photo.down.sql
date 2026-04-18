-- 050_add_flux_optimized_photo.down.sql

ALTER TABLE user_photos
    DROP COLUMN IF EXISTS flux_storage_url,
    DROP COLUMN IF EXISTS flux_storage_path;
