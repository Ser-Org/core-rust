-- 052_add_cinematic_context.down.sql

ALTER TABLE user_profiles
    DROP COLUMN IF EXISTS age_bracket,
    DROP COLUMN IF EXISTS gender,
    DROP COLUMN IF EXISTS living_situation,
    DROP COLUMN IF EXISTS industry,
    DROP COLUMN IF EXISTS career_stage,
    DROP COLUMN IF EXISTS net_worth_bracket,
    DROP COLUMN IF EXISTS income_bracket,
    DROP COLUMN IF EXISTS cinematic_context_completed;
