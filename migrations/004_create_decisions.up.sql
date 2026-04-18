-- 004_create_decisions.up.sql
-- Decision records and their AI-generated clarifying questions.

CREATE TABLE decisions (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id             UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    decision_text       TEXT NOT NULL,
    input_method        TEXT NOT NULL DEFAULT 'text',
    time_horizon_months INT NOT NULL DEFAULT 12,
    status              TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'clarifying', 'simulating', 'completed', 'failed')),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_decisions_user_id ON decisions(user_id);
CREATE INDEX idx_decisions_status ON decisions(user_id, status);

CREATE TABLE clarifying_questions (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    decision_id   UUID NOT NULL REFERENCES decisions(id) ON DELETE CASCADE,
    question_text TEXT NOT NULL,
    answer_text   TEXT NOT NULL DEFAULT '',
    answer_method TEXT NOT NULL DEFAULT '',
    sort_order    INT NOT NULL DEFAULT 0,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_clarifying_questions_decision_id ON clarifying_questions(decision_id);
