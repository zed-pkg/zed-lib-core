//! Package ownership and physical placement for the Zed registry schema.
//!
//! `zed-pkg/zed-lib-core` owns the authored DDL under `src/rust-orm/sql` and
//! publishes it as the standalone nested `zed-pkg/zed-schema` package. The old
//! shared-definitions revision remains recorded only as import provenance and
//! immutable migration identity; it is not a current dependency or
//! schema-change path.

/// Postgres schema holding the registry tables.
///
/// The registry lives in `public` with a `zed_` table prefix rather than in a
/// dedicated schema. The historical shared schema tooling keyed tables by
/// *bare* name, so a `zed` schema would have produced `orgs`, `users`, and
/// `projects` that collided with other generated adapters. Moving ownership
/// does not rename deployed tables or change that compatibility decision.
pub const ORG_SCHEMA: &str = "public";

/// Prefix shared by every registry table, and the reason a dedicated schema
/// is unnecessary.
pub const TABLE_PREFIX: &str = "zed_";

/// Source repository that owns the schema and every ORM projection.
pub const SCHEMA_REPOSITORY: &str = "zed-pkg/zed-lib-core";

/// Immutable Zed package consumed by declarative-migrations.
pub const SCHEMA_PACKAGE: &str = "zed-pkg/zed-schema";

/// Repository-relative manifest for the dependency-free schema package.
pub const SCHEMA_PACKAGE_MANIFEST: &str = "src/rust-orm/sql/.zpkg.toml";

/// Repository-relative desired-state DDL path.
pub const REGISTRY_DDL_PATH: &str = "src/rust-orm/sql/registry.sql";

/// Git blob identity at the ownership-transfer boundary.
///
/// A future schema change may update this value and the generated parity
/// artifacts together. It must never edit an already-recorded forward
/// migration or change the ledger identities below.
pub const REGISTRY_DDL_BLOB_SHA: &str = "c0869ca29c10e1c77bd9d9b8236fc61eac826ab9";

/// Package-local forward migration that adds immutable graph persistence.
pub const DEPENDENCY_GRAPH_MIGRATION_PATH: &str =
    "src/rust-orm/sql/2026-08-11-dependency-graph-artifacts.sql";

/// Immutable blob identity of the graph migration.
pub const DEPENDENCY_GRAPH_MIGRATION_BLOB_SHA: &str = "86f1b1a0b3b0d8bee26cab98aa9bf67ece738de2";

/// Historical suffix already written to deployed migration ledgers.
pub const DEPENDENCY_GRAPH_MIGRATION_IDENTITY_SUFFIX: &str =
    "a1fb823890d4a36dfab67c311f0d728d7b22c1c9";

/// Package-local forward migration that makes public visibility permanent.
pub const VISIBILITY_IMMUTABILITY_MIGRATION_PATH: &str =
    "src/rust-orm/sql/2026-08-11-public-visibility-is-permanent.sql";

/// Immutable blob identity of the public-visibility migration.
pub const VISIBILITY_IMMUTABILITY_MIGRATION_BLOB_SHA: &str =
    "8612f037dce7de6d7db66ee96db7996b33b32ea9";

/// Historical suffix already written to deployed migration ledgers.
pub const VISIBILITY_IMMUTABILITY_MIGRATION_IDENTITY_SUFFIX: &str =
    "a1fb823890d4a36dfab67c311f0d728d7b22c1c9";

// Compatibility aliases retained for existing Rust consumers. Their names
// describe historical import provenance, not current ownership.
#[doc(hidden)]
pub const SHARED_DEFS_ORG_SLICE: &str = "zed-pkg";
#[doc(hidden)]
pub const SHARED_DEFS_REVISION: &str = "a1fb823890d4a36dfab67c311f0d728d7b22c1c9";
#[doc(hidden)]
pub const SHARED_DEFS_REGISTRY_SEGMENT: &str = "pg-defs/schema/orgs/zed-pkg/registry.sql";
#[doc(hidden)]
pub const SHARED_DEFS_DEPENDENCY_GRAPH_REVISION: &str = DEPENDENCY_GRAPH_MIGRATION_IDENTITY_SUFFIX;
#[doc(hidden)]
pub const SHARED_DEFS_DEPENDENCY_GRAPH_MIGRATION: &str =
    "pg-defs/schema/orgs/zed-pkg/migrations/2026-08-11-dependency-graph-artifacts.sql";
#[doc(hidden)]
pub const SHARED_DEFS_DEPENDENCY_GRAPH_MIGRATION_BLOB_SHA: &str =
    DEPENDENCY_GRAPH_MIGRATION_BLOB_SHA;
#[doc(hidden)]
pub const SHARED_DEFS_VISIBILITY_IMMUTABILITY_REVISION: &str =
    VISIBILITY_IMMUTABILITY_MIGRATION_IDENTITY_SUFFIX;
#[doc(hidden)]
pub const SHARED_DEFS_VISIBILITY_IMMUTABILITY_MIGRATION: &str =
    "pg-defs/schema/orgs/zed-pkg/migrations/2026-08-11-public-visibility-is-permanent.sql";
#[doc(hidden)]
pub const SHARED_DEFS_VISIBILITY_IMMUTABILITY_MIGRATION_BLOB_SHA: &str =
    VISIBILITY_IMMUTABILITY_MIGRATION_BLOB_SHA;
#[doc(hidden)]
pub const SHARED_DEFS_SEA_ORM_ADAPTER: &str = "pg-defs/generated/rust/sea-orm";

/// Return a registry table name qualified for use in raw SQL.
///
/// Rejects anything that is not a plain lowercase identifier carrying the
/// registry prefix, so a caller cannot splice arbitrary text into a statement.
pub fn qualified(table: &str) -> Result<String, &'static str> {
    if !table.starts_with(TABLE_PREFIX) {
        return Err("registry table names must carry the zed_ prefix");
    }
    if table.trim() != table
        || !table
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err("invalid registry table name");
    }
    Ok(format!("{ORG_SCHEMA}.{table}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualifies_prefixed_identifiers() {
        assert_eq!(qualified("zed_projects").unwrap(), "public.zed_projects");
        assert_eq!(qualified("zed_packages").unwrap(), "public.zed_packages");
    }

    #[test]
    fn rejects_unprefixed_and_malformed_identifiers() {
        // An unprefixed name would silently address another org's table on the
        // shared instance — exactly the collision the prefix exists to prevent.
        for invalid in [
            "",
            "projects",
            "orgs",
            " zed_projects",
            "Zed_Projects",
            "zed_projects;drop",
            "zed-projects",
        ] {
            assert!(
                qualified(invalid).is_err(),
                "{invalid:?} should be rejected"
            );
        }
    }
}
