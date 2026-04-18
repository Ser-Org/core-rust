-- Add life context columns to user_profiles for household/stability onboarding step.
ALTER TABLE user_profiles
    ADD COLUMN IF NOT EXISTS relationship_status       TEXT,
    ADD COLUMN IF NOT EXISTS household_income_structure TEXT,
    ADD COLUMN IF NOT EXISTS dependent_count           INTEGER,
    ADD COLUMN IF NOT EXISTS life_stability             TEXT;
