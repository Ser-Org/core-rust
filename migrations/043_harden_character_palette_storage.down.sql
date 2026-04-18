DROP INDEX IF EXISTS idx_character_plates_user_source;

CREATE UNIQUE INDEX idx_character_plates_user_source
    ON character_plates(user_id, source_photo_id)
    WHERE status != 'failed';

UPDATE character_plates SET storage_url = '' WHERE storage_url IS NULL;
UPDATE character_plates SET storage_path = '' WHERE storage_path IS NULL;

ALTER TABLE character_plates
    ALTER COLUMN storage_url SET DEFAULT '',
    ALTER COLUMN storage_url SET NOT NULL,
    ALTER COLUMN storage_path SET DEFAULT '',
    ALTER COLUMN storage_path SET NOT NULL;

ALTER TABLE character_plates
    DROP COLUMN IF EXISTS storage_bucket,
    DROP COLUMN IF EXISTS mime_type,
    DROP COLUMN IF EXISTS attempt_count,
    DROP COLUMN IF EXISTS last_error,
    DROP COLUMN IF EXISTS last_attempt_at;

ALTER TABLE user_photos
    DROP COLUMN IF EXISTS mime_type;
