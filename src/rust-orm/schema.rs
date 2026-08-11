//! Where the registry tables live, and which reviewed contract revision they
//! came from.
//!
//! This crate never authors DDL of its own. Every table it addresses is defined
//! in `pg-defs/schema/orgs/zed-pkg/registry.sql` in the shared-definitions
//! repository at [`SHARED_DEFS_REVISION`]; [`crate::migrations`] applies that
//! reviewed SQL and nothing else.

/// Postgres schema holding the registry tables.
///
/// The registry lives in `public` with a `zed_` table prefix rather than in a
/// dedicated schema. The contract tooling in `k8s-libs-and-shared-defs` keys
/// tables by *bare* name — `sql-contract.mjs` rejects duplicates outright and
/// `generate.mjs` derives every generated identifier from it — so a `zed`
/// schema would have produced `orgs`, `users`, and `projects` that collide with
/// the `fiducia` schema across the generated adapters. See that segment's
/// README for the full rationale.
pub const ORG_SCHEMA: &str = "public";

/// Prefix shared by every registry table, and the reason a dedicated schema
/// is unnecessary.
pub const TABLE_PREFIX: &str = "zed_";

/// Organization slice consumed from the canonical shared-definitions repo.
pub const SHARED_DEFS_ORG_SLICE: &str = "zed-pkg";

/// Exact reviewed shared-definitions revision the entities correspond to.
///
/// Bump this and `shared-defs.lock.json` in the same commit, never separately.
pub const SHARED_DEFS_REVISION: &str = "d58ec90c0129151d1c09d2cf59b2804087059ef5";

/// The SQL segment that owns the registry tables.
pub const SHARED_DEFS_REGISTRY_SEGMENT: &str = "pg-defs/schema/orgs/zed-pkg/registry.sql";

/// Generated adapter location within the shared-definitions repository.
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
