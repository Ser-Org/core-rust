ALTER TABLE vault_entries
  DROP COLUMN IF EXISTS assumption_count,
  DROP COLUMN IF EXISTS top_risk_categories,
  DROP COLUMN IF EXISTS mc_p50_net_worth_delta,
  DROP COLUMN IF EXISTS category_specific_tags;
