-- 052_add_cinematic_context.up.sql
-- Adds the fields collected by the in-app "cinematic context" gate that
-- appears before a user's first Runway cinematic. The gate only fires for
-- SimulationRunType=cinematic runs; Flash is exempt. Data flows in from:
--   1. Onboarding /onboarding/photo page — writes age_bracket and gender
--      via POST /api/v1/onboarding/identity.
--   2. In-app gate modal — writes the remaining fields (living_situation,
--      industry, career_stage, net_worth_bracket, income_bracket) and reuses
--      existing user_profiles columns (relationship_status, dependent_count,
--      estimated_net_worth, estimated_yearly_salary) via POST
--      /api/v1/users/cinematic-context. Flips cinematic_context_completed
--      to true on success.
-- All new columns are nullable except the boolean flag, which defaults to
-- false. Existing users start gated and fill the form the first time they
-- trigger a cinematic.

ALTER TABLE user_profiles
    ADD COLUMN age_bracket                 TEXT,
    ADD COLUMN gender                      TEXT,
    ADD COLUMN living_situation            TEXT,
    ADD COLUMN industry                    TEXT,
    ADD COLUMN career_stage                TEXT,
    ADD COLUMN net_worth_bracket           TEXT,
    ADD COLUMN income_bracket              TEXT,
    ADD COLUMN cinematic_context_completed BOOLEAN NOT NULL DEFAULT false;
