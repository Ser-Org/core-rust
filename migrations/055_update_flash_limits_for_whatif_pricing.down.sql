-- Revert the what-if pricing update: restore the previous per-plan defaults
-- where the current value matches the post-migration default.

UPDATE subscriptions SET flash_limit = 1  WHERE plan = 'free'      AND flash_limit = 2;
UPDATE subscriptions SET flash_limit = 9  WHERE plan = 'explorer'  AND flash_limit = 10;
UPDATE subscriptions SET flash_limit = 9  WHERE plan = 'starter'   AND flash_limit = 10;
UPDATE subscriptions SET flash_limit = 90 WHERE plan = 'unlimited' AND flash_limit = 75;
UPDATE subscriptions SET flash_limit = 90 WHERE plan = 'family'    AND flash_limit = 75;
