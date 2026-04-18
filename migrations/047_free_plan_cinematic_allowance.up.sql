-- Grant free-plan users 2 cinematic sims (1 demo + 1 real decision).
UPDATE subscriptions
SET cinematic_limit = 2, updated_at = now()
WHERE plan = 'free' AND cinematic_limit = 0;
