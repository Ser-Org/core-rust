DROP INDEX IF EXISTS idx_character_plates_user_source_prompt_hash;
ALTER TABLE character_plates DROP COLUMN IF EXISTS prompt_hash;

-- Do not recreate idx_character_plates_user_source here. Once prompt-keyed plate
-- variants have been generated, multiple non-failed rows can legitimately exist
-- for one (user_id, source_photo_id), so recreating the old unique index can fail.
