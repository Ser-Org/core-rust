CREATE TABLE simulation_components (
    id UUID PRIMARY KEY,
    simulation_id UUID NOT NULL REFERENCES decision_simulations(id) ON DELETE CASCADE,
    component_key TEXT NOT NULL,
    component_type TEXT NOT NULL,
    display_name TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'running', 'completed', 'failed')),
    path TEXT,
    phase INT,
    error_code TEXT,
    error_message TEXT,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (simulation_id, component_key)
);

CREATE INDEX idx_simulation_components_simulation_id ON simulation_components(simulation_id);
CREATE INDEX idx_simulation_components_status ON simulation_components(status);
CREATE INDEX idx_simulation_components_type ON simulation_components(component_type);
