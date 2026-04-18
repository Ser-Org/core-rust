ALTER TABLE subscriptions
  ADD COLUMN cinematic_used INTEGER NOT NULL DEFAULT 0,
  ADD COLUMN cinematic_limit INTEGER NOT NULL DEFAULT 0,
  ADD COLUMN text_resim_used INTEGER NOT NULL DEFAULT 0,
  ADD COLUMN text_resim_limit INTEGER NOT NULL DEFAULT 0,
  ADD COLUMN extra_cinematic_credits INTEGER NOT NULL DEFAULT 0;

UPDATE subscriptions
SET
  cinematic_used = COALESCE(simulations_used, 0),
  cinematic_limit = CASE plan
    WHEN 'starter' THEN 2
    WHEN 'pro' THEN 5
    WHEN 'family' THEN 10
    ELSE 0
  END,
  text_resim_used = 0,
  text_resim_limit = CASE plan
    WHEN 'starter' THEN 25
    WHEN 'pro' THEN 100
    WHEN 'family' THEN 250
    ELSE 0
  END,
  extra_cinematic_credits = 0;

CREATE TABLE stripe_webhook_events (
  event_id TEXT PRIMARY KEY,
  event_type TEXT NOT NULL,
  processed_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE decision_simulations
  DROP CONSTRAINT IF EXISTS decision_simulations_decision_id_key;

ALTER TABLE decision_simulations
  ADD COLUMN run_type TEXT NOT NULL DEFAULT 'cinematic'
  CHECK (run_type IN ('cinematic', 'text_only'));

CREATE INDEX idx_decision_simulations_run_type ON decision_simulations(run_type);
