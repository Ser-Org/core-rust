-- 023_add_branch_points.down.sql
ALTER TABLE narrative_arcs DROP COLUMN IF EXISTS branch_points;
