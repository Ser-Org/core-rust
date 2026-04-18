-- Drop simulation result tables that are no longer populated.
-- The pipeline now uses decision_briefs + scenario_plans instead.
DROP TABLE IF EXISTS practical_guides CASCADE;
DROP TABLE IF EXISTS narrative_arcs CASCADE;
DROP TABLE IF EXISTS financial_projections CASCADE;
DROP TABLE IF EXISTS probability_outcomes CASCADE;
