//! Idempotent registry migrations owned by `zed-lib`.
//!
//! The migration series adopts the deployed `org` and `package` tables without
//! renaming their public URLs or destructively breaking older writers. New
//! services may use richer account-console fields while the machine package
//! API continues to insert its historical column set during the cutover.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbErr, Statement, TransactionTrait};

/// Initial account-console expansion.
pub const ACCOUNT_CONSOLE_MIGRATION: &str = "20260809_000001_account_console";
/// Compatibility migration for legacy machine-token organization claims.
pub const ORG_NAME_COMPAT_MIGRATION: &str = "20260809_000002_org_name_legacy_default";
/// Latest migration in the canonical registry series.
pub const LATEST_MIGRATION: &str = ORG_NAME_COMPAT_MIGRATION;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationReport {
    pub version: &'static str,
    pub applied: bool,
    pub applied_versions: Vec<&'static str>,
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

/// Keep historical org insert statements valid while the account console rolls
/// out. The database, not each older caller, supplies a display name from the
/// canonical slug. The trigger also repairs an explicitly blank name.
const ORG_NAME_COMPAT_SQL: &str = r#"
ALTER TABLE org ALTER COLUMN name SET DEFAULT '';
UPDATE org SET name = slug WHERE name IS NULL OR btrim(name) = '';

CREATE OR REPLACE FUNCTION zed_fill_org_name_from_slug()
RETURNS trigger
LANGUAGE plpgsql
AS $function$
BEGIN
    IF NEW.name IS NULL OR btrim(NEW.name) = '' THEN
        NEW.name := NEW.slug;
    END IF;
    RETURN NEW;
END
$function$;

DROP TRIGGER IF EXISTS zed_org_name_from_slug ON org;
CREATE TRIGGER zed_org_name_from_slug
BEFORE INSERT OR UPDATE OF slug, name ON org
FOR EACH ROW
EXECUTE FUNCTION zed_fill_org_name_from_slug();

ALTER TABLE org ALTER COLUMN name SET NOT NULL;
"#;

const MIGRATIONS: &[(&str, &str)] = &[
    (ACCOUNT_CONSOLE_MIGRATION, ACCOUNT_CONSOLE_SQL),
    (ORG_NAME_COMPAT_MIGRATION, ORG_NAME_COMPAT_SQL),
];

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

    let mut applied_versions = Vec::new();
    for (version, sql) in MIGRATIONS {
        let already_applied = txn
            .query_one(Statement::from_string(
                txn.get_database_backend(),
                format!(
                    "SELECT version FROM zed_schema_migrations WHERE version = '{}'",
                    version
                ),
            ))
            .await?
            .is_some();

        if !already_applied {
            txn.execute_unprepared(sql).await?;
            txn.execute_unprepared(&format!(
                "INSERT INTO zed_schema_migrations(version) VALUES ('{}')",
                version
            ))
            .await?;
            applied_versions.push(*version);
        }
    }

    txn.commit().await?;
    Ok(MigrationReport {
        version: LATEST_MIGRATION,
        applied: !applied_versions.is_empty(),
        applied_versions,
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

    #[test]
    fn legacy_org_claims_receive_the_slug_as_the_display_name() {
        assert!(ORG_NAME_COMPAT_SQL.contains("SET DEFAULT ''"));
        assert!(ORG_NAME_COMPAT_SQL.contains("NEW.name := NEW.slug"));
        assert!(ORG_NAME_COMPAT_SQL.contains("BEFORE INSERT"));
        assert_eq!(LATEST_MIGRATION, ORG_NAME_COMPAT_MIGRATION);
    }
}
