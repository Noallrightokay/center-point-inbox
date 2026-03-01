-- ============================================================================
-- Center Point Inbox - Initial Schema Migration
-- Migration: 001_initial_schema.sql
-- Description: Creates the foundational tables for the Center Point Inbox
--              SaaS platform - a unified AI-powered document workspace
--              integrating Google Workspace and Microsoft 365.
-- ============================================================================

BEGIN;

-- ----------------------------------------------------------------------------
-- Extensions
-- ----------------------------------------------------------------------------

CREATE EXTENSION IF NOT EXISTS "pgcrypto";   -- gen_random_uuid()
CREATE EXTENSION IF NOT EXISTS "vector";     -- pgvector for embeddings

-- ----------------------------------------------------------------------------
-- Custom ENUM types
-- ----------------------------------------------------------------------------

CREATE TYPE user_role AS ENUM ('admin', 'user', 'viewer');

CREATE TYPE oauth_provider AS ENUM ('google', 'microsoft');

CREATE TYPE subscription_tier AS ENUM ('free', 'starter', 'professional', 'enterprise');

CREATE TYPE translation_status AS ENUM ('queued', 'processing', 'completed', 'failed');

CREATE TYPE dlp_severity AS ENUM ('low', 'medium', 'high', 'critical');

-- ----------------------------------------------------------------------------
-- 1. tenants
--    Top-level organizational unit. Every user, file, and resource belongs to
--    a tenant. Created before users so the FK from users -> tenants resolves.
-- ----------------------------------------------------------------------------

CREATE TABLE tenants (
    id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    name            VARCHAR(255)    NOT NULL,
    slug            VARCHAR(63)     NOT NULL,
    settings        JSONB           NOT NULL DEFAULT '{}'::jsonb,
    subscription_tier subscription_tier NOT NULL DEFAULT 'free',
    is_active       BOOLEAN         NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ     NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ     NOT NULL DEFAULT now(),

    CONSTRAINT uq_tenants_slug UNIQUE (slug),
    CONSTRAINT ck_tenants_slug_format CHECK (slug ~ '^[a-z0-9][a-z0-9\-]{0,61}[a-z0-9]$')
);

CREATE INDEX idx_tenants_is_active ON tenants (is_active) WHERE is_active = TRUE;

-- ----------------------------------------------------------------------------
-- 2. users
-- ----------------------------------------------------------------------------

CREATE TABLE users (
    id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    email           VARCHAR(320)    NOT NULL,
    display_name    VARCHAR(255)    NOT NULL,
    role            user_role       NOT NULL DEFAULT 'user',
    tenant_id       UUID            NOT NULL
                        REFERENCES tenants (id) ON DELETE CASCADE,
    is_active       BOOLEAN         NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ     NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ     NOT NULL DEFAULT now(),

    CONSTRAINT uq_users_email_tenant UNIQUE (tenant_id, email)
);

CREATE INDEX idx_users_tenant_id  ON users (tenant_id);
CREATE INDEX idx_users_email      ON users (email);
CREATE INDEX idx_users_is_active  ON users (is_active) WHERE is_active = TRUE;

-- ----------------------------------------------------------------------------
-- 3. oauth_connections
--    Stores encrypted OAuth2 tokens for Google Workspace and Microsoft 365.
--    access_token_encrypted and refresh_token_encrypted are assumed to hold
--    values encrypted at the application layer (AES-256-GCM or similar).
-- ----------------------------------------------------------------------------

CREATE TABLE oauth_connections (
    id                      UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id                 UUID            NOT NULL
                                REFERENCES users (id) ON DELETE CASCADE,
    provider                oauth_provider  NOT NULL,
    access_token_encrypted  TEXT            NOT NULL,
    refresh_token_encrypted TEXT            NOT NULL,
    token_expires_at        TIMESTAMPTZ     NOT NULL,
    scopes                  TEXT[]          NOT NULL DEFAULT '{}',
    is_active               BOOLEAN         NOT NULL DEFAULT TRUE,
    created_at              TIMESTAMPTZ     NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ     NOT NULL DEFAULT now(),

    -- One active connection per provider per user
    CONSTRAINT uq_oauth_user_provider UNIQUE (user_id, provider)
);

CREATE INDEX idx_oauth_connections_user_id   ON oauth_connections (user_id);
CREATE INDEX idx_oauth_connections_expires   ON oauth_connections (token_expires_at)
    WHERE is_active = TRUE;

-- ----------------------------------------------------------------------------
-- 4. files_metadata
--    Canonical metadata record for every document synced from Google Drive or
--    OneDrive / SharePoint. The actual file content lives in the provider or
--    in an object store; this table tracks lineage and DLP signals.
-- ----------------------------------------------------------------------------

CREATE TABLE files_metadata (
    id                      UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id                 UUID            NOT NULL
                                REFERENCES users (id) ON DELETE CASCADE,
    tenant_id               UUID            NOT NULL
                                REFERENCES tenants (id) ON DELETE CASCADE,
    provider                oauth_provider  NOT NULL,
    provider_file_id        VARCHAR(1024)   NOT NULL,
    file_name               VARCHAR(1024)   NOT NULL,
    mime_type               VARCHAR(255)    NOT NULL,
    file_size_bytes         BIGINT,
    parent_folder_id        VARCHAR(1024),
    provider_url            TEXT,
    last_modified_provider  TIMESTAMPTZ,
    is_shared               BOOLEAN         NOT NULL DEFAULT FALSE,
    permissions_json        JSONB           NOT NULL DEFAULT '[]'::jsonb,
    phi_detected            BOOLEAN         NOT NULL DEFAULT FALSE,
    quarantined             BOOLEAN         NOT NULL DEFAULT FALSE,
    created_at              TIMESTAMPTZ     NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ     NOT NULL DEFAULT now()
);

-- Composite uniqueness: one record per provider file per tenant
CREATE UNIQUE INDEX uq_files_tenant_provider_fileid
    ON files_metadata (tenant_id, provider, provider_file_id);

CREATE INDEX idx_files_metadata_user_id       ON files_metadata (user_id);
CREATE INDEX idx_files_metadata_phi_detected  ON files_metadata (phi_detected)
    WHERE phi_detected = TRUE;
CREATE INDEX idx_files_metadata_quarantined   ON files_metadata (quarantined)
    WHERE quarantined = TRUE;

-- ----------------------------------------------------------------------------
-- 5. translation_jobs
--    Tracks document format translation requests (e.g., DOCX -> PDF,
--    Sheets -> CSV). Includes PHI redaction tracking for compliance.
-- ----------------------------------------------------------------------------

CREATE TABLE translation_jobs (
    id                  UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id             UUID            NOT NULL
                            REFERENCES users (id) ON DELETE CASCADE,
    tenant_id           UUID            NOT NULL
                            REFERENCES tenants (id) ON DELETE CASCADE,
    source_file_id      UUID            NOT NULL
                            REFERENCES files_metadata (id) ON DELETE CASCADE,
    source_format       VARCHAR(50)     NOT NULL,
    target_format       VARCHAR(50)     NOT NULL,
    status              translation_status NOT NULL DEFAULT 'queued',
    output_storage_path TEXT,
    error_message       TEXT,
    phi_redacted        BOOLEAN         NOT NULL DEFAULT FALSE,
    started_at          TIMESTAMPTZ,
    completed_at        TIMESTAMPTZ,
    created_at          TIMESTAMPTZ     NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ     NOT NULL DEFAULT now()
);

CREATE INDEX idx_translation_jobs_tenant_id      ON translation_jobs (tenant_id);
CREATE INDEX idx_translation_jobs_user_id        ON translation_jobs (user_id);
CREATE INDEX idx_translation_jobs_status         ON translation_jobs (status)
    WHERE status IN ('queued', 'processing');
CREATE INDEX idx_translation_jobs_source_file    ON translation_jobs (source_file_id);

-- ----------------------------------------------------------------------------
-- 6. audit_logs
--    Immutable append-only log of user and system actions.
--    No updated_at column - audit rows are never modified.
-- ----------------------------------------------------------------------------

CREATE TABLE audit_logs (
    id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id       UUID            NOT NULL
                        REFERENCES tenants (id) ON DELETE CASCADE,
    user_id         UUID
                        REFERENCES users (id) ON DELETE SET NULL,
    action          VARCHAR(100)    NOT NULL,
    resource_type   VARCHAR(100),
    resource_id     UUID,
    ip_address      INET,
    user_agent      TEXT,
    metadata        JSONB           NOT NULL DEFAULT '{}'::jsonb,
    created_at      TIMESTAMPTZ     NOT NULL DEFAULT now()
);

CREATE INDEX idx_audit_logs_tenant_created
    ON audit_logs (tenant_id, created_at DESC);
CREATE INDEX idx_audit_logs_user_created
    ON audit_logs (user_id, created_at DESC);
CREATE INDEX idx_audit_logs_action
    ON audit_logs (action);

-- ----------------------------------------------------------------------------
-- 7. dlp_violations
--    Records data-loss-prevention policy violations detected during file
--    scanning (e.g., SSN patterns, credit card numbers, PHI).
-- ----------------------------------------------------------------------------

CREATE TABLE dlp_violations (
    id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id       UUID            NOT NULL
                        REFERENCES tenants (id) ON DELETE CASCADE,
    user_id         UUID            NOT NULL
                        REFERENCES users (id) ON DELETE CASCADE,
    file_id         UUID
                        REFERENCES files_metadata (id) ON DELETE SET NULL,
    violation_type  VARCHAR(100)    NOT NULL,
    severity        dlp_severity    NOT NULL,
    description     TEXT            NOT NULL,
    pattern_matched TEXT,
    resolved        BOOLEAN         NOT NULL DEFAULT FALSE,
    resolved_by     UUID
                        REFERENCES users (id) ON DELETE SET NULL,
    resolved_at     TIMESTAMPTZ,
    created_at      TIMESTAMPTZ     NOT NULL DEFAULT now()
);

CREATE INDEX idx_dlp_violations_tenant_severity
    ON dlp_violations (tenant_id, severity);
CREATE INDEX idx_dlp_violations_resolved
    ON dlp_violations (resolved);
CREATE INDEX idx_dlp_violations_file_id
    ON dlp_violations (file_id) WHERE file_id IS NOT NULL;

-- ----------------------------------------------------------------------------
-- 8. embeddings
--    Stores vector embeddings for semantic search (OpenAI text-embedding-ada-002
--    or equivalent produces 1536-dimensional vectors). Uses pgvector's IVFFlat
--    index for approximate nearest-neighbor search with cosine distance.
-- ----------------------------------------------------------------------------

CREATE TABLE embeddings (
    id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id       UUID            NOT NULL
                        REFERENCES tenants (id) ON DELETE CASCADE,
    file_id         UUID            NOT NULL
                        REFERENCES files_metadata (id) ON DELETE CASCADE,
    chunk_index     INTEGER         NOT NULL,
    chunk_text      TEXT            NOT NULL,
    embedding       vector(1536)    NOT NULL,
    metadata        JSONB           NOT NULL DEFAULT '{}'::jsonb,
    created_at      TIMESTAMPTZ     NOT NULL DEFAULT now(),

    -- Each file can only have one embedding per chunk index
    CONSTRAINT uq_embeddings_file_chunk UNIQUE (file_id, chunk_index)
);

CREATE INDEX idx_embeddings_tenant_file
    ON embeddings (tenant_id, file_id);

-- IVFFlat index for approximate nearest-neighbor search using cosine distance.
-- The lists parameter should be tuned based on dataset size; 100 is a
-- reasonable starting point (rule of thumb: sqrt(row_count)).
-- NOTE: This index requires the table to have data before it can be built
-- effectively. For an empty table Postgres will accept the CREATE INDEX but
-- the index will be rebuilt on first VACUUM or REINDEX after rows are inserted.
CREATE INDEX idx_embeddings_vector_cosine
    ON embeddings
    USING ivfflat (embedding vector_cosine_ops)
    WITH (lists = 100);

-- ----------------------------------------------------------------------------
-- Trigger function: auto-update updated_at on row modification
-- ----------------------------------------------------------------------------

CREATE OR REPLACE FUNCTION set_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Apply the updated_at trigger to every table that carries the column.
-- audit_logs and embeddings are intentionally excluded (append-only).

CREATE TRIGGER trg_tenants_updated_at
    BEFORE UPDATE ON tenants
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TRIGGER trg_users_updated_at
    BEFORE UPDATE ON users
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TRIGGER trg_oauth_connections_updated_at
    BEFORE UPDATE ON oauth_connections
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TRIGGER trg_files_metadata_updated_at
    BEFORE UPDATE ON files_metadata
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TRIGGER trg_translation_jobs_updated_at
    BEFORE UPDATE ON translation_jobs
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

COMMIT;
