//! Registry migrations — the `migrate` feature, and the discrete DPM job only.
//!
//! This runner applies the reviewed `registry.sql` base segment and separately
//! versioned, forward-only compatibility migrations vendored from
//! `k8s-libs-and-shared-defs`. It never authors DDL, so the canonical contract
//! and what is actually deployed cannot drift.
//!
//! `registry.sql` is written to be idempotent — `create table if not exists`,
//! `create index if not exists`, `create or replace function` — with one
//! exception: `create trigger` and `alter table ... add constraint` are not
//! idempotent in PostgreSQL 16, so re-application is guarded by the version
//! ledger rather than by the SQL itself. The base ledger identity is therefore
//! immutable: a new shared-definitions revision must not replay that segment.

use sea_orm::{ConnectionTrait, Statement, TransactionTrait};

use crate::{
    connection::WriteContext, error::OrmError, schema::SHARED_DEFS_VISIBILITY_IMMUTABILITY_REVISION,
};

/// The vendored contract segment. Verified byte-for-byte against the
/// shared-definitions repository in CI.
const REGISTRY_SQL: &str = include_str!("sql/registry.sql");

/// Additive migration reviewed in shared definitions. It only replaces the
/// function already targeted by the existing visibility trigger, so applying
/// it cannot replay non-idempotent triggers or constraints.
const VISIBILITY_IMMUTABILITY_SQL: &str =
    include_str!("sql/2026-08-11-public-visibility-is-permanent.sql");

/// Historical ledger key emitted by the first registry migration release.
///
/// The original source-provenance constant accidentally named an unpublished
/// revision. Existing databases nevertheless recorded this exact key, so it is
/// a durable migration identity and must never be changed or replayed.
const BASE_REGISTRY_VERSION: &str = "registry@c8bdc06d74746acc6439f9527ebd02697fdf028b";

/// Version recorded in `zed_schema_migrations` once the segment is applied.
///
/// It carries the exact shared-definitions revision that introduced the latest
/// additive migration. The non-idempotent base segment retains its historical
/// ledger key independently.
pub fn registry_version() -> String {
    format!("registry-visibility-immutability@{SHARED_DEFS_VISIBILITY_IMMUTABILITY_REVISION}")
}

/// Arbitrary but stable key for the advisory lock guarding migration.
///
/// Any concurrent runner blocks here rather than racing the DDL.
const MIGRATION_LOCK_KEY: i64 = 913_447_221;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationReport {
    pub version: String,
    /// False when the ledger already recorded this version — the normal result
    /// of a redeploy, not an error.
    pub applied: bool,
}

/// Apply the registry contract exactly once, under a transaction-scoped lock.
///
/// Requires a [`WriteContext`] built from the migrator identity: the API and
/// web principals are not granted DDL, so calling this with either fails at the
/// database rather than half-applying.
pub async fn migrate(context: &WriteContext) -> Result<MigrationReport, OrmError> {
    let version = registry_version();
    let transaction = context
        .connection()
        .begin()
        .await
        .map_err(OrmError::from_db_err)?;

    // Both statements run before any read of the ledger: the lock serializes
    // concurrent migrators, and the ledger table must exist to be queried.
    transaction
        .execute_unprepared(&format!(
            "SELECT pg_advisory_xact_lock({MIGRATION_LOCK_KEY}); \
             CREATE TABLE IF NOT EXISTS zed_schema_migrations (\
               version TEXT PRIMARY KEY, \
               applied_at TIMESTAMPTZ NOT NULL DEFAULT now()\
             );"
        ))
        .await
        .map_err(OrmError::from_db_err)?;

    let base_already_applied = transaction
        .query_one(Statement::from_sql_and_values(
            transaction.get_database_backend(),
            "SELECT version FROM zed_schema_migrations WHERE version = $1",
            [BASE_REGISTRY_VERSION.into()],
        ))
        .await
        .map_err(OrmError::from_db_err)?
        .is_some();

    if !base_already_applied {
        transaction
            .execute_unprepared(REGISTRY_SQL)
            .await
            .map_err(OrmError::from_db_err)?;
        transaction
            .execute(Statement::from_sql_and_values(
                transaction.get_database_backend(),
                "INSERT INTO zed_schema_migrations(version) VALUES ($1)",
                [BASE_REGISTRY_VERSION.into()],
            ))
            .await
            .map_err(OrmError::from_db_err)?;
    }

    let patch_already_applied = transaction
        .query_one(Statement::from_sql_and_values(
            transaction.get_database_backend(),
            "SELECT version FROM zed_schema_migrations WHERE version = $1",
            [version.clone().into()],
        ))
        .await
        .map_err(OrmError::from_db_err)?
        .is_some();

    if !patch_already_applied {
        transaction
            .execute_unprepared(VISIBILITY_IMMUTABILITY_SQL)
            .await
            .map_err(OrmError::from_db_err)?;
        transaction
            .execute(Statement::from_sql_and_values(
                transaction.get_database_backend(),
                "INSERT INTO zed_schema_migrations(version) VALUES ($1)",
                [version.clone().into()],
            ))
            .await
            .map_err(OrmError::from_db_err)?;
    }

    transaction.commit().await.map_err(OrmError::from_db_err)?;

    Ok(MigrationReport {
        version,
        applied: !base_already_applied || !patch_already_applied,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_latest_version_is_pinned_to_the_additive_contract_revision() {
        assert!(registry_version().ends_with(SHARED_DEFS_VISIBILITY_IMMUTABILITY_REVISION));
        assert_ne!(registry_version(), BASE_REGISTRY_VERSION);
    }

    #[test]
    fn the_vendored_segment_defines_every_registry_table() {
        for table in [
            "zed_users",
            "zed_orgs",
            "zed_org_members",
            "zed_org_invitations",
            "zed_projects",
            "zed_project_members",
            "zed_project_invitations",
            "zed_packages",
            "zed_package_versions",
            "zed_package_licenses",
            "zed_entity_embeddings",
            "zed_package_uploads",
            "zed_package_downloads",
            "zed_api_tokens",
            "zed_audit_log",
        ] {
            assert!(
                REGISTRY_SQL.contains(&format!("create table if not exists {table} (")),
                "vendored registry.sql is missing {table}"
            );
        }
    }

    #[test]
    fn the_base_segment_retains_the_original_visibility_guard() {
        // Without the trigger the promotion rule would exist only in Rust, and
        // any other writer could bypass it.
        assert!(REGISTRY_SQL.contains("zed_packages_visibility_guard"));
        assert!(REGISTRY_SQL.contains("ZD001"));
        assert!(REGISTRY_SQL.contains("ZD002"));
        assert!(!REGISTRY_SQL.contains("ZD003"));
    }

    #[test]
    fn the_additive_visibility_migration_is_safe_to_apply_after_the_base() {
        assert!(VISIBILITY_IMMUTABILITY_SQL.contains("create or replace function"));
        assert!(VISIBILITY_IMMUTABILITY_SQL.contains("ZD003"));
        assert!(VISIBILITY_IMMUTABILITY_SQL.contains("public package % cannot become non-public"));
        for forbidden in [
            "\ncreate trigger",
            "\nalter table",
            "\ncreate table",
            "\ndrop trigger",
        ] {
            assert!(
                !VISIBILITY_IMMUTABILITY_SQL.contains(forbidden),
                "additive migration contains base DDL: {forbidden}"
            );
        }
    }

    #[test]
    fn the_vendored_segment_declares_no_schema_of_its_own() {
        // A `create schema` here would mean this crate had started authoring
        // DDL instead of applying the reviewed contract.
        assert!(!REGISTRY_SQL.contains("create schema"));
    }
}
