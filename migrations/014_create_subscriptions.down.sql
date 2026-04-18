-- 014_create_subscriptions.down.sql
DROP INDEX IF EXISTS idx_subscriptions_stripe_subscription;
DROP INDEX IF EXISTS idx_subscriptions_stripe_customer;
DROP TABLE IF EXISTS subscriptions;
