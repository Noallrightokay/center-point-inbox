-- ============================================================================
-- Center Point Inbox - Email Account Tables
-- Migration: 003_email_tables.sql
-- Description: Adds IMAP/SMTP email account connection and message tables
--              for universal email provider support (any IMAP-compatible email).
-- ============================================================================

BEGIN;

-- ----------------------------------------------------------------------------
-- 9. email_accounts
--    Stores IMAP/SMTP connection details for any email provider.
--    Credentials are AES-encrypted at the application layer.
-- ----------------------------------------------------------------------------

CREATE TABLE email_accounts (
    id                  UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id             UUID            NOT NULL
                            REFERENCES users (id) ON DELETE CASCADE,
    tenant_id           UUID            NOT NULL
                            REFERENCES tenants (id) ON DELETE CASCADE,
    email               VARCHAR(320)    NOT NULL,
    display_name        VARCHAR(255)    NOT NULL,
    provider            VARCHAR(50)     NOT NULL DEFAULT 'imap',
    imap_host           VARCHAR(255)    NOT NULL,
    imap_port           INTEGER         NOT NULL DEFAULT 993,
    smtp_host           VARCHAR(255)    NOT NULL,
    smtp_port           INTEGER         NOT NULL DEFAULT 587,
    encryption          VARCHAR(10)     NOT NULL DEFAULT 'tls',
    password_encrypted  TEXT            NOT NULL,
    is_active           BOOLEAN         NOT NULL DEFAULT TRUE,
    last_error          TEXT,
    last_sync_at        TIMESTAMPTZ,
    created_at          TIMESTAMPTZ     NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ     NOT NULL DEFAULT now(),

    CONSTRAINT uq_email_accounts_tenant_email UNIQUE (tenant_id, email)
);

CREATE INDEX idx_email_accounts_user ON email_accounts (user_id);
CREATE INDEX idx_email_accounts_active ON email_accounts (is_active) WHERE is_active = TRUE;

-- ----------------------------------------------------------------------------
-- 10. email_messages
--    Cached IMAP messages for fast inbox rendering. The source of truth
--    remains the IMAP server; this table is a local cache.
-- ----------------------------------------------------------------------------

CREATE TABLE email_messages (
    id                  UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    account_id          UUID            NOT NULL
                            REFERENCES email_accounts (id) ON DELETE CASCADE,
    tenant_id           UUID            NOT NULL
                            REFERENCES tenants (id) ON DELETE CASCADE,
    user_id             UUID            NOT NULL
                            REFERENCES users (id) ON DELETE CASCADE,
    message_uid         VARCHAR(255)    NOT NULL,
    subject             VARCHAR(1024)   NOT NULL,
    snippet             TEXT,
    body_html           TEXT,
    body_text           TEXT,
    from_address        VARCHAR(320)    NOT NULL,
    from_name           VARCHAR(255),
    to_addresses        TEXT[]          NOT NULL DEFAULT '{}',
    cc_addresses        TEXT[]          NOT NULL DEFAULT '{}',
    bcc_addresses       TEXT[]          NOT NULL DEFAULT '{}',
    labels              TEXT[]          NOT NULL DEFAULT '{}',
    is_read             BOOLEAN         NOT NULL DEFAULT FALSE,
    is_starred          BOOLEAN         NOT NULL DEFAULT FALSE,
    has_attachments     BOOLEAN         NOT NULL DEFAULT FALSE,
    thread_id           VARCHAR(255),
    in_reply_to         VARCHAR(255),
    size_bytes          BIGINT,
    received_at         TIMESTAMPTZ     NOT NULL,
    created_at          TIMESTAMPTZ     NOT NULL DEFAULT now(),

    CONSTRAINT uq_email_messages_account_uid UNIQUE (account_id, message_uid)
);

CREATE INDEX idx_email_messages_tenant_received ON email_messages (tenant_id, received_at DESC);
CREATE INDEX idx_email_messages_user ON email_messages (user_id, received_at DESC);
CREATE INDEX idx_email_messages_account ON email_messages (account_id, received_at DESC);

-- Apply updated_at trigger
CREATE TRIGGER trg_email_accounts_updated_at
    BEFORE UPDATE ON email_accounts
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

COMMIT;
