-- Rollback: Restore old category names.
-- Note: This is best-effort — "Career & Education Pivots" cannot distinguish
-- original Career vs Education, so we default to Career.

UPDATE decisions SET category = 'Career'
  WHERE category = 'Career & Education Pivots';

UPDATE decisions SET category = 'Real Estate'
  WHERE category = 'Housing & Major Purchases';

UPDATE decisions SET category = 'Finance'
  WHERE category = 'Financial Milestones & Investments';

UPDATE decisions SET category = 'Health'
  WHERE category = 'Health, Wellness & Personal Overhauls';

UPDATE decisions SET category = 'Relationships'
  WHERE category = 'Family, Relationships & Life Stage Changes';

UPDATE decisions SET category = 'Other'
  WHERE category = 'Relocation & Lifestyle Shifts';

-- Vault entries
UPDATE vault_entries SET category = 'Career'
  WHERE category = 'Career & Education Pivots';

UPDATE vault_entries SET category = 'Real Estate'
  WHERE category = 'Housing & Major Purchases';

UPDATE vault_entries SET category = 'Finance'
  WHERE category = 'Financial Milestones & Investments';

UPDATE vault_entries SET category = 'Health'
  WHERE category = 'Health, Wellness & Personal Overhauls';

UPDATE vault_entries SET category = 'Relationships'
  WHERE category = 'Family, Relationships & Life Stage Changes';

UPDATE vault_entries SET category = 'Other'
  WHERE category = 'Relocation & Lifestyle Shifts';
