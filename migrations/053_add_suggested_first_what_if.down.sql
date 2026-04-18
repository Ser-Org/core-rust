-- 053_add_suggested_first_what_if.down.sql

ALTER TABLE user_profiles
    DROP COLUMN IF EXISTS suggested_first_what_if,
    DROP COLUMN IF EXISTS suggested_first_what_if_generated_at;
