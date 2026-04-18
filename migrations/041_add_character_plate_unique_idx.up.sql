-- Prevent duplicate character plates for the same user+source photo.
-- Failed plates are excluded so that a retry can create a new record.
CREATE UNIQUE INDEX idx_character_plates_user_source
    ON character_plates(user_id, source_photo_id)
    WHERE status != 'failed';
