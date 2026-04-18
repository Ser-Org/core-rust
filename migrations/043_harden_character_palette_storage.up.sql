ALTER TABLE user_photos
    ADD COLUMN mime_type TEXT NOT NULL DEFAULT 'image/jpeg';

ALTER TABLE character_plates
    ADD COLUMN storage_bucket TEXT,
    ADD COLUMN mime_type TEXT,
    ADD COLUMN attempt_count INT NOT NULL DEFAULT 0,
    ADD COLUMN last_error TEXT,
    ADD COLUMN last_attempt_at TIMESTAMPTZ;

UPDATE character_plates SET storage_url = NULL WHERE storage_url = '';
UPDATE character_plates SET storage_path = NULL WHERE storage_path = '';

ALTER TABLE character_plates
    ALTER COLUMN storage_url DROP NOT NULL,
    ALTER COLUMN storage_url DROP DEFAULT,
    ALTER COLUMN storage_path DROP NOT NULL,
    ALTER COLUMN storage_path DROP DEFAULT;

UPDATE character_plates
SET attempt_count = 1
WHERE attempt_count = 0;

DROP INDEX IF EXISTS idx_character_plates_user_source;

CREATE UNIQUE INDEX idx_character_plates_user_source
    ON character_plates(user_id, source_photo_id);
