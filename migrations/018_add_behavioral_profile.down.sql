ALTER TABLE user_profiles
  DROP COLUMN IF EXISTS risk_tolerance,
  DROP COLUMN IF EXISTS follow_through,
  DROP COLUMN IF EXISTS optimism_bias,
  DROP COLUMN IF EXISTS stress_response,
  DROP COLUMN IF EXISTS decision_style;
