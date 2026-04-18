-- 014_create_subscriptions.up.sql
-- Stores Stripe subscription state per user.
-- One row per user (UNIQUE on user_id). Free users get a row with plan='free'.

CREATE TABLE subscriptions (
    id                     UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id                UUID        NOT NULL UNIQUE REFERENCES users(id) ON DELETE CASCADE,
    stripe_customer_id     TEXT        UNIQUE,
    stripe_subscription_id TEXT        UNIQUE,
    plan                   TEXT        NOT NULL DEFAULT 'free',
    status                 TEXT        NOT NULL DEFAULT 'active',
    simulations_used       INTEGER     NOT NULL DEFAULT 0,
    simulations_limit      INTEGER     NOT NULL DEFAULT 2,
    overage_price_cents    INTEGER     NOT NULL DEFAULT 0,
    period_start           TIMESTAMPTZ,
    period_end             TIMESTAMPTZ,
    cancel_at_period_end   BOOLEAN     NOT NULL DEFAULT false,
    created_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at             TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_subscriptions_stripe_customer     ON subscriptions(stripe_customer_id);
CREATE INDEX idx_subscriptions_stripe_subscription ON subscriptions(stripe_subscription_id);
