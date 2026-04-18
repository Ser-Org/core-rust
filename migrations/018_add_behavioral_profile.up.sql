ALTER TABLE user_profiles
  ADD COLUMN IF NOT EXISTS risk_tolerance TEXT,
  ADD COLUMN IF NOT EXISTS follow_through TEXT,
  ADD COLUMN IF NOT EXISTS optimism_bias TEXT,
  ADD COLUMN IF NOT EXISTS stress_response TEXT,
  ADD COLUMN IF NOT EXISTS decision_style TEXT;
