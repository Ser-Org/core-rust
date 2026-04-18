ALTER TABLE user_profiles
    DROP COLUMN IF EXISTS life_stability,
    DROP COLUMN IF EXISTS dependent_count,
    DROP COLUMN IF EXISTS household_income_structure,
    DROP COLUMN IF EXISTS relationship_status;
