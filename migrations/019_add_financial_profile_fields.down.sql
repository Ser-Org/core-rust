ALTER TABLE user_profiles
  DROP COLUMN IF EXISTS saving_habits,
  DROP COLUMN IF EXISTS debt_comfort,
  DROP COLUMN IF EXISTS housing_stability,
  DROP COLUMN IF EXISTS income_stability;
