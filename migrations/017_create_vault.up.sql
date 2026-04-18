-- 017_create_vault.up.sql
-- Public Decision Vault: anonymized, pre-processed snapshots of completed decisions.

CREATE TABLE vault_entries (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    decision_id         UUID NOT NULL UNIQUE REFERENCES decisions(id) ON DELETE CASCADE,
    anonymized_teaser   TEXT NOT NULL,
    category            TEXT NOT NULL DEFAULT '',
    time_horizon_months INT NOT NULL,
    best_case_probability   NUMERIC NOT NULL DEFAULT 0,
    likely_case_probability NUMERIC NOT NULL DEFAULT 0,
    worst_case_probability  NUMERIC NOT NULL DEFAULT 0,
    best_case_summary       TEXT NOT NULL DEFAULT '',
    likely_case_summary     TEXT NOT NULL DEFAULT '',
    worst_case_summary      TEXT NOT NULL DEFAULT '',
    decision_risk_score     INT NOT NULL DEFAULT 0,
    confidence_spread       NUMERIC NOT NULL DEFAULT 0,
    narrative_snippet       TEXT NOT NULL DEFAULT '',
    key_risks_json          JSONB NOT NULL DEFAULT '[]',
    non_obvious_json        JSONB NOT NULL DEFAULT '[]',
    net_worth_start         NUMERIC,
    net_worth_end_likely    NUMERIC,
    net_worth_end_best      NUMERIC,
    net_worth_end_worst     NUMERIC,
    view_count              INT NOT NULL DEFAULT 0,
    save_count              INT NOT NULL DEFAULT 0,
    simulate_count          INT NOT NULL DEFAULT 0,
    trending_score          NUMERIC NOT NULL DEFAULT 0,
    flagged                 BOOLEAN NOT NULL DEFAULT false,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_vault_entries_category ON vault_entries(category);
CREATE INDEX idx_vault_entries_risk ON vault_entries(decision_risk_score);
CREATE INDEX idx_vault_entries_trending ON vault_entries(trending_score DESC);
CREATE INDEX idx_vault_entries_created ON vault_entries(created_at DESC);
CREATE INDEX idx_vault_entries_horizon ON vault_entries(time_horizon_months);
CREATE INDEX idx_vault_entries_not_flagged ON vault_entries(id) WHERE flagged = false;
CREATE INDEX idx_vault_entries_saves ON vault_entries(save_count DESC);

CREATE TABLE vault_aggregates (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    stat_key        TEXT NOT NULL UNIQUE,
    stat_value      JSONB NOT NULL DEFAULT '{}',
    computed_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE saved_ideas (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    vault_entry_id  UUID NOT NULL REFERENCES vault_entries(id) ON DELETE CASCADE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(user_id, vault_entry_id)
);

CREATE INDEX idx_saved_ideas_user ON saved_ideas(user_id, created_at DESC);
