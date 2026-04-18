UPDATE subscriptions
SET cinematic_limit = 0, updated_at = now()
WHERE plan = 'free' AND cinematic_limit = 2;
