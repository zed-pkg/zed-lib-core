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
    connection::WriteContext,
    error::OrmError,
    schema::{SHARED_DEFS_DEPENDENCY_GRAPH_REVISION, SHARED_DEFS_VISIBILITY_IMMUTABILITY_REVISION},
};

/// The vendored contract segment. Verified byte-for-byte against the
/// shared-definitions repository in CI.
const REGISTRY_SQL: &str = include_str!("sql/registry.sql");

/// Forward-only graph persistence migration reviewed in shared definitions.
///
/// Fresh databases receive the same desired state from `registry.sql`, then
/// execute this idempotent migration so every database records the same ordered
/// ledger. Existing databases skip the base and receive these tables directly.
const DEPENDENCY_GRAPH_SQL: &str = include_str!("sql/2026-08-11-dependency-graph-artifacts.sql");

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

/// Ledger identity for the forward-only dependency-graph upgrade.
pub fn dependency_graph_version() -> String {
    format!("registry-dependency-graph-artifacts@{SHARED_DEFS_DEPENDENCY_GRAPH_REVISION}")
}

/// Ledger identity for the forward-only public-visibility upgrade.
pub fn visibility_immutability_version() -> String {
    format!("registry-visibility-immutability@{SHARED_DEFS_VISIBILITY_IMMUTABILITY_REVISION}")
}

/// Target version retained for compatibility with existing migration callers.
///
/// The runner owns an ordered ledger rather than one mutable schema version.
/// This value is the final step in that order; use
/// [`dependency_graph_version`] and [`visibility_immutability_version`] when an
/// individual migration identity is required.
pub fn registry_version() -> String {
    visibility_immutability_version()
}

/// Arbitrary but stable key for the advisory lock guarding migration.
///
/// Any concurrent runner blocks here rather than racing the DDL.
const MIGRATION_LOCK_KEY: i64 = 913_447_221;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MigrationStep {
    HistoricalBase,
    DependencyGraph,
    VisibilityImmutability,
}

impl MigrationStep {
    /// Deployment order is part of the contract. Never substitute a changing
    /// shared-definitions revision for the historical base identity.
    const ORDERED: [Self; 3] = [
        Self::HistoricalBase,
        Self::DependencyGraph,
        Self::VisibilityImmutability,
    ];

    fn version(self) -> String {
        match self {
            Self::HistoricalBase => BASE_REGISTRY_VERSION.to_owned(),
            Self::DependencyGraph => dependency_graph_version(),
            Self::VisibilityImmutability => visibility_immutability_version(),
        }
    }

    fn sql(self) -> &'static str {
        match self {
            Self::HistoricalBase => REGISTRY_SQL,
            Self::DependencyGraph => DEPENDENCY_GRAPH_SQL,
            Self::VisibilityImmutability => VISIBILITY_IMMUTABILITY_SQL,
        }
    }
}

fn migration_plan(
    ledger_has_entries: bool,
    registry_has_base_table: bool,
    recorded_versions: &[String],
) -> Vec<MigrationStep> {
    // A base segment is only safe on an empty ledger. An unrecognized entry can
    // come from an older release, so its presence is enough to prove that the
    // database was initialized and the non-idempotent historical DDL must not
    // be replayed. Forward migrations remain independently discoverable.
    let database_was_initialized =
        ledger_has_entries || registry_has_base_table || !recorded_versions.is_empty();
    MigrationStep::ORDERED
        .into_iter()
        .filter(|step| match step {
            MigrationStep::HistoricalBase => !database_was_initialized,
            _ => {
                let version = step.version();
                !recorded_versions.contains(&version)
            }
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationReport {
    /// Final target identity in the ordered migration series.
    pub version: String,
    /// False when the ledger already recorded every migration — the normal
    /// result of a redeploy, not an error.
    pub applied: bool,
}

/// Apply the registry contract exactly once, under a transaction-scoped lock.
///
/// Requires a [`WriteContext`] built from the migrator identity: the API and
/// web principals are not granted DDL, so calling this with either fails at the
/// database rather than half-applying.
pub async fn migrate(context: &WriteContext) -> Result<MigrationReport, OrmError> {
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

    let ledger_has_entries = transaction
        .query_one(Statement::from_string(
            transaction.get_database_backend(),
            "SELECT version FROM zed_schema_migrations LIMIT 1",
        ))
        .await
        .map_err(OrmError::from_db_err)?
        .is_some();

    // The table marker also protects databases initialized before the ledger
    // existed (or by the declarative migration controller). Replaying the base
    // against either one would collide with its triggers and constraints.
    let registry_has_base_table = transaction
        .query_one(Statement::from_string(
            transaction.get_database_backend(),
            "SELECT 1 \
               FROM pg_catalog.pg_class AS relation \
               JOIN pg_catalog.pg_namespace AS namespace \
                 ON namespace.oid = relation.relnamespace \
              WHERE namespace.nspname = 'public' \
                AND relation.relname = 'zed_users' \
                AND relation.relkind IN ('r', 'p') \
              LIMIT 1",
        ))
        .await
        .map_err(OrmError::from_db_err)?
        .is_some();

    let mut recorded_versions = Vec::new();
    for step in MigrationStep::ORDERED {
        let version = step.version();
        let is_recorded = transaction
            .query_one(Statement::from_sql_and_values(
                transaction.get_database_backend(),
                "SELECT version FROM zed_schema_migrations WHERE version = $1",
                [version.clone().into()],
            ))
            .await
            .map_err(OrmError::from_db_err)?
            .is_some();
        if is_recorded {
            recorded_versions.push(version);
        }
    }

    let pending = migration_plan(
        ledger_has_entries,
        registry_has_base_table,
        &recorded_versions,
    );
    for step in &pending {
        let version = step.version();
        transaction
            .execute_unprepared(step.sql())
            .await
            .map_err(OrmError::from_db_err)?;
        transaction
            .execute(Statement::from_sql_and_values(
                transaction.get_database_backend(),
                "INSERT INTO zed_schema_migrations(version) VALUES ($1)",
                [version.into()],
            ))
            .await
            .map_err(OrmError::from_db_err)?;
    }

    transaction.commit().await.map_err(OrmError::from_db_err)?;

    Ok(MigrationReport {
        version: registry_version(),
        applied: !pending.is_empty(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_forward_migration_has_an_independent_ledger_identity() {
        assert!(dependency_graph_version().ends_with(SHARED_DEFS_DEPENDENCY_GRAPH_REVISION));
        assert!(registry_version().ends_with(SHARED_DEFS_VISIBILITY_IMMUTABILITY_REVISION));
        assert_eq!(registry_version(), visibility_immutability_version());
        assert_ne!(
            dependency_graph_version(),
            visibility_immutability_version()
        );
        assert_ne!(dependency_graph_version(), BASE_REGISTRY_VERSION);
        assert_ne!(registry_version(), BASE_REGISTRY_VERSION);
    }

    #[test]
    fn a_fresh_database_runs_the_base_then_each_forward_migration() {
        assert_eq!(
            migration_plan(false, false, &[]),
            MigrationStep::ORDERED.to_vec()
        );
    }

    #[test]
    fn an_existing_base_is_never_replayed() {
        let plan = migration_plan(true, true, &[BASE_REGISTRY_VERSION.to_owned()]);
        assert_eq!(
            plan,
            vec![
                MigrationStep::DependencyGraph,
                MigrationStep::VisibilityImmutability
            ]
        );
        assert!(!plan.contains(&MigrationStep::HistoricalBase));
    }

    #[test]
    fn an_unrecognized_historical_ledger_entry_also_prevents_base_replay() {
        let plan = migration_plan(true, true, &["registry@legacy-release".to_owned()]);
        assert_eq!(
            plan,
            vec![
                MigrationStep::DependencyGraph,
                MigrationStep::VisibilityImmutability
            ]
        );
        assert!(!plan.contains(&MigrationStep::HistoricalBase));
    }

    #[test]
    fn a_pre_ledger_registry_table_prevents_base_replay() {
        let plan = migration_plan(false, true, &[]);
        assert_eq!(
            plan,
            vec![
                MigrationStep::DependencyGraph,
                MigrationStep::VisibilityImmutability
            ]
        );
        assert!(!plan.contains(&MigrationStep::HistoricalBase));
    }

    #[test]
    fn a_database_with_the_visibility_patch_still_receives_the_graph_upgrade() {
        let plan = migration_plan(
            true,
            true,
            &[
                BASE_REGISTRY_VERSION.to_owned(),
                visibility_immutability_version(),
            ],
        );
        assert_eq!(plan, vec![MigrationStep::DependencyGraph]);
    }

    #[test]
    fn a_repeated_run_has_no_pending_work() {
        let ledger = MigrationStep::ORDERED
            .into_iter()
            .map(MigrationStep::version)
            .collect::<Vec<_>>();
        assert!(migration_plan(true, true, &ledger).is_empty());
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
            "zed_dependency_graph_artifacts",
            "zed_dependency_graph_edges",
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
        assert!(REGISTRY_SQL.contains("zed_dependency_graph_artifacts_document_binding_chk"));
        assert!(REGISTRY_SQL.contains("zed_dependency_graph_artifacts_immutable"));
        assert!(REGISTRY_SQL.contains("ZD003"));
        assert!(REGISTRY_SQL.contains("ZD004"));
        assert!(REGISTRY_SQL.contains("ZD005"));
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
    fn the_graph_migration_is_retry_safe_and_does_not_mix_visibility_policy() {
        for table in [
            "zed_dependency_graph_artifacts",
            "zed_dependency_graph_edges",
        ] {
            assert!(
                DEPENDENCY_GRAPH_SQL.contains(&format!("create table if not exists {table} (")),
                "dependency-graph migration is missing {table}"
            );
        }
        for constraint in [
            "zed_dependency_graph_artifacts_root_version_fk",
            "zed_dependency_graph_edges_artifact_fk",
            "zed_dependency_graph_edges_from_package_fk",
            "zed_dependency_graph_edges_from_version_fk",
            "zed_dependency_graph_edges_to_package_fk",
            "zed_dependency_graph_edges_to_version_fk",
        ] {
            assert!(DEPENDENCY_GRAPH_SQL.contains(&format!("conname = '{constraint}'")));
            assert!(DEPENDENCY_GRAPH_SQL.contains(&format!("add constraint {constraint}")));
        }
        assert_eq!(DEPENDENCY_GRAPH_SQL.matches("do $zed_fk$").count(), 6);
        assert!(DEPENDENCY_GRAPH_SQL.contains("sealed_at timestamptz"));
        assert!(
            DEPENDENCY_GRAPH_SQL.contains("zed_dependency_graph_artifacts_document_binding_chk")
        );
        assert!(DEPENDENCY_GRAPH_SQL.contains("zed_dependency_graph_artifacts_immutable"));
        assert!(DEPENDENCY_GRAPH_SQL.contains("zed_dependency_graph_edges_immutable"));
        assert!(DEPENDENCY_GRAPH_SQL.contains("must be inserted unsealed"));
        assert!(DEPENDENCY_GRAPH_SQL.contains("ZD004"));
        assert!(DEPENDENCY_GRAPH_SQL.contains("ZD005"));
        assert!(!DEPENDENCY_GRAPH_SQL.contains("ZD003"));
        assert!(!VISIBILITY_IMMUTABILITY_SQL.contains("zed_dependency_graph_artifacts"));
        assert!(!VISIBILITY_IMMUTABILITY_SQL.contains("zed_dependency_graph_edges"));
    }

    #[test]
    fn the_vendored_segment_declares_no_schema_of_its_own() {
        // A `create schema` here would mean this crate had started authoring
        // DDL instead of applying the reviewed contract.
        assert!(!REGISTRY_SQL.contains("create schema"));
    }
}
