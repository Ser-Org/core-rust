-- 013_add_share_token.down.sql

DROP INDEX IF EXISTS idx_decisions_share_token;
ALTER TABLE decisions DROP COLUMN IF EXISTS share_token;
