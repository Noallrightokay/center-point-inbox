-- ============================================================================
-- Center Point Inbox - Seed Data Migration
-- Migration: 002_seed_data.sql
-- Description: Seeds the database with a default tenant, a system admin user,
--              and foundational reference data required for the platform to
--              operate after a fresh deployment.
-- ============================================================================

BEGIN;

-- ----------------------------------------------------------------------------
-- Default Tenant
-- ----------------------------------------------------------------------------
-- The "Center Point" tenant serves as the platform-level organization.
-- Customer tenants are provisioned through the application's onboarding flow.

INSERT INTO tenants (id, name, slug, settings, subscription_tier, is_active)
VALUES (
    '00000000-0000-4000-a000-000000000001',
    'Center Point',
    'center-point',
    jsonb_build_object(
        'features', jsonb_build_object(
            'dlp_scanning',       true,
            'phi_detection',      true,
            'translation_jobs',   true,
            'semantic_search',    true,
            'audit_logging',      true
        ),
        'limits', jsonb_build_object(
            'max_users',          100,
            'max_file_size_mb',   500,
            'max_storage_gb',     1000
        ),
        'branding', jsonb_build_object(
            'primary_color',  '#2563EB',
            'logo_url',       NULL
        )
    ),
    'enterprise',
    TRUE
);

-- ----------------------------------------------------------------------------
-- Seed Users (one per role for the default tenant)
-- ----------------------------------------------------------------------------
-- These seed users demonstrate the three available roles. In production the
-- passwords / auth tokens are managed through the OAuth flow, so no password
-- column exists. The admin account can be mapped to an SSO identity during
-- first-time setup.

INSERT INTO users (id, email, display_name, role, tenant_id, is_active)
VALUES
    (
        '00000000-0000-4000-b000-000000000001',
        'admin@centerpoint.dev',
        'System Administrator',
        'admin',
        '00000000-0000-4000-a000-000000000001',
        TRUE
    ),
    (
        '00000000-0000-4000-b000-000000000002',
        'user@centerpoint.dev',
        'Default User',
        'user',
        '00000000-0000-4000-a000-000000000001',
        TRUE
    ),
    (
        '00000000-0000-4000-b000-000000000003',
        'viewer@centerpoint.dev',
        'Default Viewer',
        'viewer',
        '00000000-0000-4000-a000-000000000001',
        TRUE
    );

-- ----------------------------------------------------------------------------
-- Seed Audit Log Entry
-- ----------------------------------------------------------------------------
-- Record the initial provisioning event so the audit trail has a clear origin.

INSERT INTO audit_logs (tenant_id, user_id, action, resource_type, resource_id, metadata)
VALUES (
    '00000000-0000-4000-a000-000000000001',
    '00000000-0000-4000-b000-000000000001',
    'tenant.provisioned',
    'tenant',
    '00000000-0000-4000-a000-000000000001',
    jsonb_build_object(
        'migration',    '002_seed_data.sql',
        'description',  'Initial platform provisioning via database seed migration',
        'seeded_roles', jsonb_build_array('admin', 'user', 'viewer')
    )
);

COMMIT;
