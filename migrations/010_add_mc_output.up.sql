ALTER TABLE financial_projections
ADD COLUMN mc_assumptions JSONB,
ADD COLUMN mc_aggregated JSONB;

COMMENT ON COLUMN financial_projections.mc_assumptions IS 'Claude-generated simulation parameters used as Monte Carlo inputs';
COMMENT ON COLUMN financial_projections.mc_aggregated IS 'Aggregated Monte Carlo output (percentile curves, risk frequencies)';
