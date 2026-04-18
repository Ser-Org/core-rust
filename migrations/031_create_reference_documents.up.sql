-- Phase 4: Reference data grounding — track uploaded reference documents.
-- Documents themselves live in Supabase Storage (scout-reference bucket).
-- This table records metadata for admin management and freshness tracking.

CREATE TABLE reference_documents (
  id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  storage_path     TEXT NOT NULL UNIQUE,        -- e.g. "mortgage/freddie-mac-pmms-2026.md"
  bucket           TEXT NOT NULL DEFAULT 'scout-reference',
  title            TEXT NOT NULL,
  category         TEXT NOT NULL,               -- matches reference category folders
  content_type     TEXT NOT NULL,               -- "text/markdown", "text/plain", "application/pdf"
  description      TEXT,                        -- human description of what this doc contains
  last_refreshed_at TIMESTAMPTZ,               -- when content was last updated
  active           BOOLEAN NOT NULL DEFAULT true,
  created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_ref_docs_category ON reference_documents(category) WHERE active = true;
CREATE INDEX idx_ref_docs_active   ON reference_documents(active) WHERE active = true;
