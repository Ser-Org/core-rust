DROP INDEX IF EXISTS idx_decision_simulations_run_type;

ALTER TABLE decision_simulations
  DROP COLUMN IF EXISTS run_type;

ALTER TABLE decision_simulations
  ADD CONSTRAINT decision_simulations_decision_id_key UNIQUE (decision_id);

DROP TABLE IF EXISTS stripe_webhook_events;

ALTER TABLE subscriptions
  DROP COLUMN IF EXISTS extra_cinematic_credits,
  DROP COLUMN IF EXISTS text_resim_limit,
  DROP COLUMN IF EXISTS text_resim_used,
  DROP COLUMN IF EXISTS cinematic_limit,
  DROP COLUMN IF EXISTS cinematic_used;
