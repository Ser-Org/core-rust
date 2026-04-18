-- Phase 0: Migrate decision categories from 7 (Scout) to 6 (Grok-aligned).
--
-- Old: Career, Education, Real Estate, Finance, Health, Relationships, Other
-- New: Relocation & Lifestyle Shifts, Housing & Major Purchases,
--      Career & Education Pivots, Family Relationships & Life Stage Changes,
--      Financial Milestones & Investments, Health Wellness & Personal Overhauls

-- Decisions table
UPDATE decisions SET category = 'Career & Education Pivots'
  WHERE category IN ('Career', 'Education');

UPDATE decisions SET category = 'Housing & Major Purchases'
  WHERE category = 'Real Estate';

UPDATE decisions SET category = 'Financial Milestones & Investments'
  WHERE category = 'Finance';

UPDATE decisions SET category = 'Health, Wellness & Personal Overhauls'
  WHERE category = 'Health';

UPDATE decisions SET category = 'Family, Relationships & Life Stage Changes'
  WHERE category = 'Relationships';

-- Best-effort: reclassify "Other" decisions that look like relocation.
UPDATE decisions SET category = 'Relocation & Lifestyle Shifts'
  WHERE category = 'Other'
    AND decision_text ~* '(move|relocat|city|country|abroad|immigrat|emigrat|neighborhood)';

-- Remaining "Other" → Career & Education Pivots as a safe fallback.
UPDATE decisions SET category = 'Career & Education Pivots'
  WHERE category = 'Other';

-- Vault entries (same mapping)
UPDATE vault_entries SET category = 'Career & Education Pivots'
  WHERE category IN ('Career', 'Education');

UPDATE vault_entries SET category = 'Housing & Major Purchases'
  WHERE category = 'Real Estate';

UPDATE vault_entries SET category = 'Financial Milestones & Investments'
  WHERE category = 'Finance';

UPDATE vault_entries SET category = 'Health, Wellness & Personal Overhauls'
  WHERE category = 'Health';

UPDATE vault_entries SET category = 'Family, Relationships & Life Stage Changes'
  WHERE category = 'Relationships';

UPDATE vault_entries SET category = 'Relocation & Lifestyle Shifts'
  WHERE category = 'Other'
    AND anonymized_teaser ~* '(move|relocat|city|country|abroad|immigrat|emigrat|neighborhood)';

UPDATE vault_entries SET category = 'Career & Education Pivots'
  WHERE category = 'Other';
