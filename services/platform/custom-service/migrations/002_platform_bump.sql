-- Phase 4.4 upgrade rehearsal — additive platform migrate/bump.
-- Simulates a platform upgrade that must not break installed custom packages.

INSERT INTO custom_platform_meta (key, value)
VALUES ('schema_version', '2')
ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value;

CREATE TABLE IF NOT EXISTS custom_platform_upgrade_marker (
    id INT PRIMARY KEY DEFAULT 1,
    upgraded_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    note TEXT NOT NULL DEFAULT 'phase44_rehearsal'
);
