-- 053_add_suggested_first_what_if.up.sql
-- Adds storage for the AI-generated "first what-if" question produced after
-- onboarding completion. Mirrors the existing suggested_first_decision
-- pattern (migration 012) but for the Flash what-if feature instead of the
-- Runway cinematic pipeline.
--
-- The column holds a JSONB blob matching suggestedFirstWhatIfResponse in
-- orchestration/onboarding_orchestrator.go:
--   { "question": "What if ...", "rationale": "..." }
-- The _generated_at timestamp lets the GET handler distinguish "not yet
-- produced" (NULL) from "produced and cached".

ALTER TABLE user_profiles
    ADD COLUMN suggested_first_what_if              JSONB,
    ADD COLUMN suggested_first_what_if_generated_at TIMESTAMPTZ;
