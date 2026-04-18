-- 012_add_suggested_first_decision.up.sql
-- Adds nullable columns to user_profiles for storing the AI-generated
-- suggested first decision shown on the /first-decision screen.

ALTER TABLE user_profiles
ADD COLUMN suggested_first_decision JSONB,
ADD COLUMN suggested_first_decision_generated_at TIMESTAMPTZ;
