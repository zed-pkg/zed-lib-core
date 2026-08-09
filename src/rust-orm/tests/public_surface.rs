use std::{fs, path::PathBuf};

/// The crate root: `src/rust-orm` inside the zed-lib-core repository.
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The repository root, which owns `.zpkg.toml` and `shared-defs.lock.json`
/// for every language slice rather than duplicating them per crate.
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
    assert!(library.contains("#[cfg(feature = \"read-write\")]\npub use connection"));
    assert!(library.contains("compile_fail"));

    let cargo = read("Cargo.toml");
    assert!(cargo.contains("default = [\"read-only\"]"));
    assert!(cargo.contains("read-write = [\"read-only\"]"));
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
fn shared_schema_source_is_exact_and_external() {
    let lock = read_repo("shared-defs.lock.json");
    for contract in [
        "d8fb884023a26de79d4f5d533f486a2d3dbec7cc",
        "\"org_slice\": \"zed-pkg\"",
        "\"schema\": \"public\"",
        "\"table_prefix\": \"zed_\"",
        "pg-defs/schema/orgs/zed-pkg/registry.sql",
        "pg-defs/generated/rust/sea-orm",
    ] {
        assert!(lock.contains(contract), "shared-defs lock lost {contract}");
    }

    let zpkg = read_repo(".zpkg.toml");
    assert!(zpkg.contains("\"oresoftware/k8s-libs-and-shared-defs\""));
}

#[test]
fn live_denial_probe_remains_available_but_opt_in() {
    let connection = read("connection.rs");
    assert!(connection
        .contains("#[ignore = \"requires a dedicated ORM_CORE_TEST_DATABASE_URL database\"]"));
    assert!(connection.contains("live_read_only_context_rejects_schema_ddl"));
    assert!(connection.contains("read-only context unexpectedly executed DDL"));
}
