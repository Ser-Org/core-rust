CREATE TABLE IF NOT EXISTS decision_working_sessions (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    decision_id   UUID NOT NULL REFERENCES decisions(id) ON DELETE CASCADE,
    simulation_id UUID NOT NULL REFERENCES decision_simulations(id) ON DELETE CASCADE,
    user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    status        TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'finalizing', 'complete', 'failed')),
    current_turn  INT NOT NULL DEFAULT 0,
    is_read_only  BOOLEAN NOT NULL DEFAULT false,
    last_error    TEXT,
    completed_at  TIMESTAMPTZ,
    finalized_at  TIMESTAMPTZ,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT decision_working_sessions_simulation_unique UNIQUE (simulation_id)
);

CREATE INDEX idx_decision_working_sessions_decision_id
    ON decision_working_sessions(decision_id);

CREATE INDEX idx_decision_working_sessions_user_id
    ON decision_working_sessions(user_id);

CREATE TABLE IF NOT EXISTS decision_working_session_turns (
    id                      UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    session_id              UUID NOT NULL REFERENCES decision_working_sessions(id) ON DELETE CASCADE,
    turn_number             INT NOT NULL,
    turn_type               TEXT NOT NULL,
    system_payload          JSONB NOT NULL DEFAULT '{}',
    raw_ai_response         TEXT NOT NULL DEFAULT '',
    user_response_type      TEXT,
    user_response_option_id TEXT,
    user_response_label     TEXT,
    user_response_text      TEXT,
    user_response_metadata  JSONB NOT NULL DEFAULT '{}',
    branch_explorations     JSONB NOT NULL DEFAULT '[]',
    presented_at            TIMESTAMPTZ,
    responded_at            TIMESTAMPTZ,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT decision_working_session_turns_unique UNIQUE (session_id, turn_number)
);

CREATE INDEX idx_decision_working_session_turns_session_id
    ON decision_working_session_turns(session_id);

CREATE TABLE IF NOT EXISTS decision_working_session_memories (
    id                    UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    decision_id           UUID NOT NULL REFERENCES decisions(id) ON DELETE CASCADE,
    simulation_id         UUID NOT NULL REFERENCES decision_simulations(id) ON DELETE CASCADE,
    session_id            UUID NOT NULL REFERENCES decision_working_sessions(id) ON DELETE CASCADE,
    user_id               UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    source_turn           INT NOT NULL DEFAULT 0,
    applied_to_life_state BOOLEAN NOT NULL DEFAULT false,
    payload               JSONB NOT NULL DEFAULT '{}',
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at            TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_decision_working_session_memories_user_id
    ON decision_working_session_memories(user_id);

CREATE INDEX idx_decision_working_session_memories_session_id
    ON decision_working_session_memories(session_id);

CREATE TABLE IF NOT EXISTS decision_checkpoint_tracks (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    session_id      UUID NOT NULL REFERENCES decision_working_sessions(id) ON DELETE CASCADE,
    decision_id     UUID NOT NULL REFERENCES decisions(id) ON DELETE CASCADE,
    simulation_id   UUID NOT NULL REFERENCES decision_simulations(id) ON DELETE CASCADE,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    checkpoint_id   TEXT NOT NULL,
    label           TEXT NOT NULL,
    target_date     TIMESTAMPTZ NOT NULL,
    recipient_email TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'scheduled' CHECK (status IN ('scheduled', 'sent', 'failed', 'cancelled')),
    sent_at         TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT decision_checkpoint_tracks_unique UNIQUE (session_id, checkpoint_id)
);

CREATE INDEX idx_decision_checkpoint_tracks_user_id
    ON decision_checkpoint_tracks(user_id);

CREATE INDEX idx_decision_checkpoint_tracks_target_date
    ON decision_checkpoint_tracks(target_date);
