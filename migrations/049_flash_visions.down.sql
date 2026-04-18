ALTER TABLE subscriptions
    DROP COLUMN IF EXISTS flash_limit,
    DROP COLUMN IF EXISTS flash_used;

DROP TABLE IF EXISTS flash_images;
DROP TABLE IF EXISTS flash_visions;
