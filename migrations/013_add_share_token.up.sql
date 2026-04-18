-- 013_add_share_token.up.sql
-- Adds a unique, publicly-safe share token to decisions for link sharing.

ALTER TABLE decisions ADD COLUMN share_token TEXT UNIQUE;

CREATE INDEX idx_decisions_share_token ON decisions(share_token);
