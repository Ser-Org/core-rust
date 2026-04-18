-- 007_create_dashboard_snapshots.up.sql
-- Scout main dashboard snapshots, regenerated on onboarding completion or manual refresh.

CREATE TABLE scout_dashboard_snapshots (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id              UUID NOT NULL UNIQUE REFERENCES users(id) ON DELETE CASCADE,
    financial_trajectory JSONB NOT NULL DEFAULT '{}',
    life_momentum_score  JSONB NOT NULL DEFAULT '{}',
    probability_outlook  JSONB NOT NULL DEFAULT '{}',
    narrative_summary    TEXT NOT NULL DEFAULT '',
    raw_ai_response      TEXT NOT NULL DEFAULT '',
    generated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_scout_dashboard_user_id ON scout_dashboard_snapshots(user_id);
