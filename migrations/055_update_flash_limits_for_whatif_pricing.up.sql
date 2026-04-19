-- Align existing subscriptions with the new what-if pricing structure.
-- New limits: Free=2, Explorer/Starter=10, Pro=30, Unlimited/Family=75.
-- Only touches rows where the current limit matches the old per-plan default,
-- so any manually-tuned limits (ops overrides, grandfathered users) survive.

UPDATE subscriptions SET flash_limit = 2  WHERE plan = 'free'      AND flash_limit = 1;
UPDATE subscriptions SET flash_limit = 10 WHERE plan = 'explorer'  AND flash_limit = 9;
UPDATE subscriptions SET flash_limit = 10 WHERE plan = 'starter'   AND flash_limit = 9;
UPDATE subscriptions SET flash_limit = 75 WHERE plan = 'unlimited' AND flash_limit = 90;
UPDATE subscriptions SET flash_limit = 75 WHERE plan = 'family'    AND flash_limit = 90;
-- Pro stays at 30 — no update needed.
