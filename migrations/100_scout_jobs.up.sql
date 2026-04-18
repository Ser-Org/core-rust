CREATE TABLE IF NOT EXISTS scout_jobs (
    id           UUID PRIMARY KEY,
    kind         TEXT NOT NULL,
    args         JSONB NOT NULL DEFAULT '{}'::jsonb,
    state        TEXT NOT NULL DEFAULT 'pending',
    scheduled_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    attempts     INT NOT NULL DEFAULT 0,
    max_attempts INT NOT NULL DEFAULT 3,
    last_error   TEXT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_scout_jobs_ready
    ON scout_jobs (scheduled_at)
    WHERE state = 'pending';

CREATE INDEX IF NOT EXISTS idx_scout_jobs_kind_state
    ON scout_jobs (kind, state);
