ALTER TABLE user_profiles
  ADD COLUMN IF NOT EXISTS saving_habits TEXT,
  ADD COLUMN IF NOT EXISTS debt_comfort TEXT,
  ADD COLUMN IF NOT EXISTS housing_stability TEXT,
  ADD COLUMN IF NOT EXISTS income_stability TEXT;
