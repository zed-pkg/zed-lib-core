//! Idempotent registry migrations owned by `zed-lib`.
//!
//! The first migration adopts the deployed `org` and `package` tables without
//! renaming their public URLs or destructive constraints. It adds the account
//! console graph around them and records completion under an advisory lock.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbErr, Statement, TransactionTrait};

/// Immutable migration identifier stored in `zed_schema_migrations`.
pub const ACCOUNT_CONSOLE_MIGRATION: &str = "20260809_000001_account_console";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationReport {
    pub version: &'static str,
    pub applied: bool,
}

const ACCOUNT_CONSOLE_SQL: &str = r#"
CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE IF NOT EXISTS users (
    id UUID PRIMARY KEY,
    shared_auth_subject TEXT NOT NULL UNIQUE,
    email TEXT,
    display_name TEXT,
    avatar_url TEXT,
    settings JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS org (
    id UUID PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE org ADD COLUMN IF NOT EXISTS name TEXT;
ALTER TABLE org ADD COLUMN IF NOT EXISTS description TEXT;
ALTER TABLE org ADD COLUMN IF NOT EXISTS settings JSONB NOT NULL DEFAULT '{}'::jsonb;
ALTER TABLE org ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT now();
UPDATE org SET name = slug WHERE name IS NULL OR btrim(name) = '';
ALTER TABLE org ALTER COLUMN name SET NOT NULL;

CREATE TABLE IF NOT EXISTS org_members (
    org_id UUID NOT NULL REFERENCES org(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'member', 'reader')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (org_id, user_id)
);
CREATE INDEX IF NOT EXISTS idx_org_members_user ON org_members(user_id, org_id);

CREATE TABLE IF NOT EXISTS org_invitations (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES org(id) ON DELETE CASCADE,
    invited_by_user_id UUID NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    email TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('admin', 'member', 'reader')),
    token_hash TEXT NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    accepted_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_org_invitations_org_email
    ON org_invitations(org_id, lower(email));

CREATE TABLE IF NOT EXISTS projects (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES org(id) ON DELETE CASCADE,
    slug TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    settings JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, slug)
);
CREATE INDEX IF NOT EXISTS idx_projects_org ON projects(org_id, slug);

CREATE TABLE IF NOT EXISTS project_members (
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'member', 'reader')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, user_id)
);
CREATE INDEX IF NOT EXISTS idx_project_members_user
    ON project_members(user_id, project_id);

CREATE TABLE IF NOT EXISTS project_invitations (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    invited_by_user_id UUID NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    email TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('admin', 'member', 'reader')),
    token_hash TEXT NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    accepted_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_project_invitations_project_email
    ON project_invitations(project_id, lower(email));

CREATE TABLE IF NOT EXISTS package (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES org(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT,
    vcs TEXT NOT NULL DEFAULT 'git',
    repo_url TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE package ADD COLUMN IF NOT EXISTS project_id UUID;
ALTER TABLE package ADD COLUMN IF NOT EXISTS visibility TEXT NOT NULL DEFAULT 'public';
ALTER TABLE package ADD COLUMN IF NOT EXISTS config JSONB NOT NULL DEFAULT '{}'::jsonb;
ALTER TABLE package ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT now();

DO $migration$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'fk_package_project'
    ) THEN
        ALTER TABLE package
            ADD CONSTRAINT fk_package_project
            FOREIGN KEY (project_id)
            REFERENCES projects(id)
            ON DELETE SET NULL;
    END IF;
END
$migration$;

CREATE UNIQUE INDEX IF NOT EXISTS idx_package_org_name
    ON package(org_id, name);
CREATE INDEX IF NOT EXISTS idx_package_project
    ON package(project_id) WHERE project_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_package_visibility
    ON package(visibility);
"#;

/// Apply every zed-lib migration exactly once under a transaction-scoped lock.
pub async fn migrate(conn: &DatabaseConnection) -> Result<MigrationReport, DbErr> {
    let txn = conn.begin().await?;
    txn.execute_unprepared(
        "SELECT pg_advisory_xact_lock(913447221); \
         CREATE TABLE IF NOT EXISTS zed_schema_migrations (\
           version TEXT PRIMARY KEY,\
           applied_at TIMESTAMPTZ NOT NULL DEFAULT now()\
         );",
    )
    .await?;

    let already_applied = txn
        .query_one(Statement::from_string(
            txn.get_database_backend(),
            format!(
                "SELECT version FROM zed_schema_migrations WHERE version = '{}'",
                ACCOUNT_CONSOLE_MIGRATION
            ),
        ))
        .await?
        .is_some();

    if !already_applied {
        txn.execute_unprepared(ACCOUNT_CONSOLE_SQL).await?;
        txn.execute_unprepared(&format!(
            "INSERT INTO zed_schema_migrations(version) VALUES ('{}')",
            ACCOUNT_CONSOLE_MIGRATION
        ))
        .await?;
    }

    txn.commit().await?;
    Ok(MigrationReport {
        version: ACCOUNT_CONSOLE_MIGRATION,
        applied: !already_applied,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_contains_the_requested_registry_entities() {
        for table in [
            "users",
            "org",
            "projects",
            "package",
            "org_members",
            "project_members",
        ] {
            assert!(
                ACCOUNT_CONSOLE_SQL.contains(table),
                "migration is missing {table}"
            );
        }
        for column in ["shared_auth_subject", "project_id", "visibility", "config"] {
            assert!(
                ACCOUNT_CONSOLE_SQL.contains(column),
                "migration is missing {column}"
            );
        }
    }

    #[test]
    fn package_urls_remain_org_scoped() {
        assert!(ACCOUNT_CONSOLE_SQL.contains("idx_package_org_name"));
        assert!(!ACCOUNT_CONSOLE_SQL.contains("UNIQUE (project_id, name)"));
    }
}
