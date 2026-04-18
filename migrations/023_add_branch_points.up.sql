-- 023_add_branch_points.up.sql
-- Add branch_points JSONB column to narrative_arcs for branching narrative support.
-- Existing rows get an empty array default, preserving backward compatibility.

ALTER TABLE narrative_arcs
    ADD COLUMN branch_points JSONB NOT NULL DEFAULT '[]';
