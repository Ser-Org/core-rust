-- Phase 7: Granular Vault & Comparison — enrich vault entries for filtering.

ALTER TABLE vault_entries
  ADD COLUMN assumption_count INT,
  ADD COLUMN top_risk_categories TEXT[],
  ADD COLUMN mc_p50_net_worth_delta NUMERIC,
  ADD COLUMN category_specific_tags TEXT[];

CREATE INDEX idx_vault_entries_nw_delta ON vault_entries(mc_p50_net_worth_delta)
  WHERE mc_p50_net_worth_delta IS NOT NULL;
