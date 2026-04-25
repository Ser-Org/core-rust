DROP INDEX IF EXISTS idx_character_plates_user_source;

ALTER TABLE character_plates
    ADD COLUMN IF NOT EXISTS prompt_hash TEXT GENERATED ALWAYS AS (md5(prompt_used)) STORED;

CREATE UNIQUE INDEX IF NOT EXISTS idx_character_plates_user_source_prompt_hash
    ON character_plates(user_id, source_photo_id, prompt_hash)
    WHERE status != 'failed';
