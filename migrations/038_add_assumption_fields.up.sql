-- Add kind (premise/prediction), grounding (stated/derived/inferred/speculative),
-- and evidence_refs to simulation_assumptions for the audit remediation.
ALTER TABLE simulation_assumptions ADD COLUMN kind TEXT;
ALTER TABLE simulation_assumptions ADD COLUMN grounding TEXT;
ALTER TABLE simulation_assumptions ADD COLUMN evidence_refs TEXT[];
