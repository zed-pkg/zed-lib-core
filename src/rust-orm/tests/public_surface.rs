use std::{fs, path::PathBuf};

/// The crate root: `src/rust-orm` inside the zed-lib-core repository.
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The repository root, which owns the root package, nested schema package,
/// and historical import record for every language slice.
fn repo_root() -> PathBuf {
    root()
        .parent()
        .and_then(|path| path.parent())
        .expect("crate lives two levels below the repository root")
        .to_path_buf()
}

fn read_repo(relative: &str) -> String {
    fs::read_to_string(repo_root().join(relative))
        .unwrap_or_else(|error| panic!("failed to read {relative}: {error}"))
}

fn read(relative: &str) -> String {
    fs::read_to_string(root().join(relative))
        .unwrap_or_else(|error| panic!("failed to read {relative}: {error}"))
}

#[test]
fn raw_orm_types_are_not_reexported() {
    let library = read("lib.rs");
    for forbidden in [
        "pub use sea_orm",
        "pub type ReadContext = DatabaseConnection",
        "pub type WriteContext = DatabaseConnection",
    ] {
        assert!(
            !library.contains(forbidden),
            "public surface leaked {forbidden}"
        );
    }

    let connection = read("connection.rs");
    assert!(connection.contains("connection: DatabaseConnection"));
    assert!(connection.contains("pub(crate) fn connection"));
    assert!(!connection.contains("pub fn connection(&self)"));
}

#[test]
fn every_write_symbol_is_feature_gated() {
    let library = read("lib.rs");
    assert!(library.contains("#[cfg(feature = \"read-write\")]\npub mod write;"));
    assert!(library.contains("#[cfg(feature = \"read-write\")]\npub mod public_intake;"));
    assert!(library.contains("#[cfg(feature = \"read-write\")]\npub use connection"));
    assert!(library.contains("compile_fail"));

    let cargo = read("Cargo.toml");
    assert!(cargo.contains("default = [\"read-only\"]"));
    assert!(cargo.contains("read-write = [\"read-only\"]"));
    assert!(cargo.contains(
        "zed-interfaces = { git = \"https://github.com/zed-pkg/zed-interfaces.git\", rev = \"70508770bc76bb4ea59a2e6da57a3c416044e96c\" }"
    ));
    assert!(!cargo.contains("../../../zed-interfaces"));
}

#[test]
fn organization_invitations_use_one_typed_write_command() {
    let write = read("write.rs");
    assert!(write.contains("pub struct OrgInvitationInput<'a>"));
    assert!(write.contains("invitation: OrgInvitationInput<'_>"));
    assert!(write.contains("let OrgInvitationInput {"));
    assert!(!write.contains("allow(clippy::too_many_arguments)"));
}

#[test]
fn product_schema_is_local_and_historical_import_provenance_is_exact() {
    let lock = read_repo("shared-defs.lock.json");
    for contract in [
        "\"mode\": \"historical-import-only\"",
        "\"current_authority\": \"zed-pkg/zed-lib-core:src/rust-orm/sql\"",
        "a1fb823890d4a36dfab67c311f0d728d7b22c1c9",
        "c0869ca29c10e1c77bd9d9b8236fc61eac826ab9",
        "86f1b1a0b3b0d8bee26cab98aa9bf67ece738de2",
        "8612f037dce7de6d7db66ee96db7996b33b32ea9",
        "\"org_slice\": \"zed-pkg\"",
        "\"schema\": \"public\"",
        "\"table_prefix\": \"zed_\"",
        "pg-defs/schema/orgs/zed-pkg/registry.sql",
        "2026-08-11-dependency-graph-artifacts.sql",
        "2026-08-11-public-visibility-is-permanent.sql",
        "pg-defs/generated/rust/sea-orm",
    ] {
        assert!(lock.contains(contract), "shared-defs lock lost {contract}");
    }

    let zpkg = read_repo(".zpkg.toml");
    assert!(!zpkg.contains("\"oresoftware/k8s-libs-and-shared-defs\""));
    assert!(!zpkg.contains("[targets.sql-schema]"));
    assert!(!zpkg.contains("[targets.rust-orm]"));

    let orm_zpkg = read_repo("src/rust-orm/.zpkg.toml");
    assert!(orm_zpkg.contains("name = \"zed-orm-core\""));
    assert!(orm_zpkg.contains("\"zed-pkg/zed-interfaces\" = \"^0.1.0\""));
    assert!(orm_zpkg.contains("adapter = \"rust\""));
    assert!(orm_zpkg.contains("orm-package-smoke.sh"));

    let schema_zpkg = read_repo("src/rust-orm/sql/.zpkg.toml");
    assert!(schema_zpkg.contains("name = \"zed-schema\""));
    assert!(schema_zpkg.contains("adapter = \"none\""));
    assert!(schema_zpkg.contains("schema-package-smoke.sh"));
    assert!(!schema_zpkg.contains("[dependencies]"));

    assert_eq!(zed_orm_core::SCHEMA_REPOSITORY, "zed-pkg/zed-lib-core");
    assert_eq!(zed_orm_core::SCHEMA_PACKAGE, "zed-pkg/zed-schema");
    assert_eq!(
        zed_orm_core::SCHEMA_PACKAGE_MANIFEST,
        "src/rust-orm/sql/.zpkg.toml"
    );
    assert_eq!(
        zed_orm_core::REGISTRY_DDL_PATH,
        "src/rust-orm/sql/registry.sql"
    );
    assert_eq!(
        zed_orm_core::DEPENDENCY_GRAPH_MIGRATION_PATH,
        "src/rust-orm/sql/2026-08-11-dependency-graph-artifacts.sql"
    );
    assert_eq!(
        zed_orm_core::VISIBILITY_IMMUTABILITY_MIGRATION_PATH,
        "src/rust-orm/sql/2026-08-11-public-visibility-is-permanent.sql"
    );

    assert_eq!(
        zed_orm_core::SHARED_DEFS_DEPENDENCY_GRAPH_REVISION,
        "a1fb823890d4a36dfab67c311f0d728d7b22c1c9"
    );
    assert_eq!(
        zed_orm_core::SHARED_DEFS_DEPENDENCY_GRAPH_MIGRATION,
        "pg-defs/schema/orgs/zed-pkg/migrations/2026-08-11-dependency-graph-artifacts.sql"
    );
    assert_eq!(
        zed_orm_core::SHARED_DEFS_DEPENDENCY_GRAPH_MIGRATION_BLOB_SHA,
        "86f1b1a0b3b0d8bee26cab98aa9bf67ece738de2"
    );
    assert_eq!(
        zed_orm_core::SHARED_DEFS_VISIBILITY_IMMUTABILITY_REVISION,
        "a1fb823890d4a36dfab67c311f0d728d7b22c1c9"
    );
    assert_eq!(
        zed_orm_core::SHARED_DEFS_VISIBILITY_IMMUTABILITY_MIGRATION,
        "pg-defs/schema/orgs/zed-pkg/migrations/2026-08-11-public-visibility-is-permanent.sql"
    );
    assert_eq!(
        zed_orm_core::SHARED_DEFS_VISIBILITY_IMMUTABILITY_MIGRATION_BLOB_SHA,
        "8612f037dce7de6d7db66ee96db7996b33b32ea9"
    );
}

#[test]
fn migration_ledger_keeps_all_forward_steps_distinct() {
    let migrations = read("migrations.rs");
    assert!(migrations.contains("registry@c8bdc06d74746acc6439f9527ebd02697fdf028b"));
    assert!(migrations.contains("Self::HistoricalBase"));
    assert!(migrations.contains("Self::DependencyGraph"));
    assert!(migrations.contains("Self::VisibilityImmutability"));
    assert!(migrations.contains("Self::PublicIntake"));
    assert!(migrations.contains("sql/2026-08-11-dependency-graph-artifacts.sql"));
    assert!(migrations.contains("sql/2026-08-11-public-visibility-is-permanent.sql"));
    assert!(migrations.contains("sql/2026-09-02-public-intake.sql"));
}

#[cfg(feature = "migrate")]
#[test]
fn individual_migration_versions_are_public_and_distinct() {
    let graph = zed_orm_core::migrations::dependency_graph_version();
    let visibility = zed_orm_core::migrations::visibility_immutability_version();
    let intake = zed_orm_core::migrations::public_intake_version();
    assert!(graph.ends_with(zed_orm_core::DEPENDENCY_GRAPH_MIGRATION_IDENTITY_SUFFIX));
    assert!(visibility.ends_with(zed_orm_core::VISIBILITY_IMMUTABILITY_MIGRATION_IDENTITY_SUFFIX));
    assert!(intake.starts_with("registry-public-intake@"));
    assert_ne!(graph, visibility);
    assert_ne!(visibility, intake);
    assert_ne!(graph, intake);
    assert_eq!(zed_orm_core::migrations::registry_version(), intake);
}

#[test]
fn dependency_graph_writes_are_gated_and_reads_are_visibility_scoped() {
    let registry = read("registry/mod.rs");
    assert!(registry.contains("#[cfg(feature = \"read-write\")]\npub use graphs"));

    let graphs = read("registry/graphs.rs");
    assert!(graphs.contains("pub async fn dependency_graph_by_digest"));
    assert!(graphs.contains("pub async fn incoming_dependency_edges"));
    assert!(graphs.contains("pub async fn outgoing_dependency_edges"));
    assert!(graphs.contains("visible_org_ids"));
    assert!(graphs.contains("package::Column::Visibility.eq(\"public\")"));
    assert!(graphs.contains("pub async fn persist_dependency_graph"));
    assert!(graphs.contains("#[cfg(feature = \"read-write\")]"));
}

#[test]
fn live_denial_probe_remains_available_but_opt_in() {
    let connection = read("connection.rs");
    assert!(connection
        .contains("#[ignore = \"requires a dedicated ORM_CORE_TEST_DATABASE_URL database\"]"));
    assert!(connection.contains("live_read_only_context_rejects_schema_ddl"));
    assert!(connection.contains("read-only context unexpectedly executed DDL"));
}

#[test]
fn exact_project_reads_are_on_the_default_surface() {
    let _project = zed_orm_core::read::project_by_org_and_slug;
    let _project_id = zed_orm_core::read::project_by_id;
    let _query = zed_orm_core::read::project_role_for_user;
    let _version = zed_orm_core::read::package_version_by_package_and_version;
}
