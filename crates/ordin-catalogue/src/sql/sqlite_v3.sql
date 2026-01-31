-- spec: ordin-desktop/components/CatalogueService#CAT-BND-1
-- The bundled catalogue's digest, beside ordin_version; bundled row only.
ALTER TABLE agent_repositories ADD COLUMN bundled_digest TEXT;
