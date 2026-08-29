#!/usr/bin/env python3
"""Fail-closed certification for the zed-lib-core semantic merge."""

from __future__ import annotations

import json
import pathlib
import re
import subprocess
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[1]
MERGE = "f27f72cc65640407409d38953c8d30ee4c95f3a6"
SEMANTIC_FOLD = "9fdc5fed96b707b99b3b02e6541060831c3d70fd"
PARENTS = (
    "430aafe24b6c3ab1263f1351ab4941545f592f19",
    "a5dabf3685db94ffdf5ae30cb3b3e4cc1cce298f",
)
EXPECTED_SHARED_DEFS_REVISION = "a1fb823890d4a36dfab67c311f0d728d7b22c1c9"
EXPECTED_REGISTRY_BLOB = "c0869ca29c10e1c77bd9d9b8236fc61eac826ab9"
EXPECTED_DEPENDENCY_GRAPH_REVISION = "a1fb823890d4a36dfab67c311f0d728d7b22c1c9"
EXPECTED_DEPENDENCY_GRAPH_BLOB = "86f1b1a0b3b0d8bee26cab98aa9bf67ece738de2"
EXPECTED_VISIBILITY_REVISION = "a1fb823890d4a36dfab67c311f0d728d7b22c1c9"
EXPECTED_VISIBILITY_BLOB = "8612f037dce7de6d7db66ee96db7996b33b32ea9"
EXPECTED_PACKAGE = "zed-pkg/zed-lib-core"
EXPECTED_ORM_PACKAGE = "zed-pkg/zed-orm-core"
EXPECTED_SCHEMA_PACKAGE = "zed-pkg/zed-schema"
EXPECTED_SCHEMA_AUTHORITY = "zed-pkg/zed-lib-core:src/rust-orm/sql"
EXPECTED_ORM_MANIFEST = "src/rust-orm/.zpkg.toml"
EXPECTED_SCHEMA_MANIFEST = "src/rust-orm/sql/.zpkg.toml"

DEPENDENCY_GRAPH_MIGRATION = (
    "pg-defs/schema/orgs/zed-pkg/migrations/"
    "2026-08-11-dependency-graph-artifacts.sql"
)
VENDORED_DEPENDENCY_GRAPH_MIGRATION = (
    "src/rust-orm/sql/2026-08-11-dependency-graph-artifacts.sql"
)
VISIBILITY_MIGRATION = (
    "pg-defs/schema/orgs/zed-pkg/migrations/"
    "2026-08-11-public-visibility-is-permanent.sql"
)
VENDORED_VISIBILITY_MIGRATION = (
    "src/rust-orm/sql/2026-08-11-public-visibility-is-permanent.sql"
)


def fail(message: str) -> "NoReturn":
    raise SystemExit(f"zed-lib-core merge contract: {message}")


def read_json(path: str) -> dict:
    value = json.loads((ROOT / path).read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        fail(f"{path} must contain an object")
    return value


def git(*args: str) -> str:
    return subprocess.check_output(
        ["git", "-C", str(ROOT), *args], text=True, stderr=subprocess.DEVNULL
    ).strip()


def rust_string_const(source: str, name: str) -> str:
    match = re.search(
        rf"pub const {re.escape(name)}: &str =\s*\"([^\"]+)\";",
        source,
    )
    if match is None:
        fail(f"Rust schema constant is missing or is not a string: {name}")
    return match.group(1)


def assert_history() -> None:
    for commit in (MERGE, SEMANTIC_FOLD, *PARENTS):
        try:
            git("cat-file", "-e", f"{commit}^{{commit}}")
        except subprocess.CalledProcessError:
            fail(f"required history commit is missing: {commit}")
    merge_parents = git("show", "-s", "--format=%P", MERGE).split()
    if tuple(merge_parents) != PARENTS:
        fail(f"merge parents differ: {merge_parents!r}")
    if subprocess.run(
        ["git", "-C", str(ROOT), "merge-base", "--is-ancestor", MERGE, "HEAD"],
        check=False,
    ).returncode:
        fail("two-history merge is not an ancestor of HEAD")
    if subprocess.run(
        ["git", "-C", str(ROOT), "merge-base", "--is-ancestor", SEMANTIC_FOLD, "HEAD"],
        check=False,
    ).returncode:
        fail("semantic fold is not an ancestor of HEAD")


def assert_package() -> None:
    manifest = tomllib.loads((ROOT / ".zpkg.toml").read_text(encoding="utf-8"))
    package = manifest.get("package", {})
    identity = f"{package.get('org')}/{package.get('name')}"
    if identity != EXPECTED_PACKAGE or package.get("version") != "0.1.0":
        fail(f"unexpected package identity: {identity}@{package.get('version')}")
    # The root target follows the current zed-interfaces manifest model and is
    # canonically named `repository`; older manifests used ad-hoc names such as
    # `whole`, which are now rejected by the parser.
    required_targets = {
        "repository",
        "rust",
        "conformance",
        "dart",
        "typescript",
    }
    targets = set(manifest.get("targets", {}))
    if targets != required_targets:
        fail(f"target set differs: {sorted(targets)}")
    for target, spec in manifest["targets"].items():
        directory = ROOT / spec["dir"]
        if not directory.is_dir():
            fail(f"target {target} directory is missing: {directory}")

    if {"rust-orm", "sql-schema"} & targets:
        fail("ORM and schema packages must not inherit root target metadata")

    orm_manifest = tomllib.loads((ROOT / EXPECTED_ORM_MANIFEST).read_text(encoding="utf-8"))
    orm_package = orm_manifest.get("package", {})
    orm_identity = f"{orm_package.get('org')}/{orm_package.get('name')}"
    if orm_identity != EXPECTED_ORM_PACKAGE:
        fail(f"unexpected ORM package identity: {orm_identity}")
    if orm_package.get("version") != package.get("version"):
        fail("ORM package version differs from the source release")
    if orm_manifest.get("dependencies") != {"zed-pkg/zed-interfaces": "^0.1.0"}:
        fail("ORM package dependency boundary differs")
    if orm_manifest.get("install", {}).get("adapter") != "rust":
        fail("ORM package adapter must remain rust")
    if orm_manifest.get("publish", {}).get("smoke_test") != (
        'sh "$ZED_PKG_TEST_TARGET/orm-package-smoke.sh"'
    ):
        fail("ORM package consumer smoke test differs")

    schema_manifest = tomllib.loads((ROOT / EXPECTED_SCHEMA_MANIFEST).read_text(encoding="utf-8"))
    schema_package = schema_manifest.get("package", {})
    schema_identity = f"{schema_package.get('org')}/{schema_package.get('name')}"
    if schema_identity != EXPECTED_SCHEMA_PACKAGE:
        fail(f"unexpected schema package identity: {schema_identity}")
    if schema_package.get("version") != package.get("version"):
        fail("schema package version differs from the source release")
    if schema_manifest.get("dependencies"):
        fail("schema package must not have runtime package dependencies")
    if schema_manifest.get("install", {}).get("adapter") != "none":
        fail("schema package adapter must remain none")
    if schema_manifest.get("publish", {}).get("smoke_test") != (
        'sh "$ZED_PKG_TEST_TARGET/schema-package-smoke.sh"'
    ):
        fail("schema package consumer smoke test differs")

    if (ROOT / ".zpkg.lock").exists():
        lock = tomllib.loads((ROOT / ".zpkg.lock").read_text(encoding="utf-8"))
        packages = lock.get("package", [])
        if not packages:
            fail("a committed lock cannot be version-only or empty")
        for item in packages:
            for field in ("sha256", "size", "format", "vcs_tag", "vcs_commit", "source"):
                if field not in item:
                    fail(f"lock entry is missing {field}")
            if not re.fullmatch(r"[0-9a-f]{64}", item["sha256"]):
                fail("lock contains an invalid SHA-256")
            if item["sha256"] == "0" * 64:
                fail("lock contains an all-zero SHA-256")
            if not isinstance(item["size"], int) or item["size"] <= 0:
                fail("lock contains an invalid artifact size")
            if item["format"] not in {"tar.gz", "tar.zst", "zip"}:
                fail("lock contains an unsupported artifact format")
            if not re.fullmatch(r"[0-9a-f]{40}", item["vcs_commit"]):
                fail("lock contains an invalid VCS commit")
            if not item["vcs_tag"] or not item["source"]:
                fail("lock contains empty immutable provenance")


def assert_schema_ownership() -> None:
    lock = read_json("shared-defs.lock.json")
    if lock.get("mode") != "historical-import-only":
        fail("shared-definitions record is not historical-only")
    if lock.get("current_authority") != EXPECTED_SCHEMA_AUTHORITY:
        fail("current schema authority differs")
    if lock.get("revision") != EXPECTED_SHARED_DEFS_REVISION:
        fail("shared-definitions revision differs")
    if lock.get("registry_blob_sha") != EXPECTED_REGISTRY_BLOB:
        fail("registry blob identity differs")
    if lock.get("dependency_graph_revision") != EXPECTED_DEPENDENCY_GRAPH_REVISION:
        fail("dependency-graph migration revision differs")
    if lock.get("dependency_graph_migration") != DEPENDENCY_GRAPH_MIGRATION:
        fail("dependency-graph canonical migration path differs")
    if lock.get("dependency_graph_migration_blob_sha") != EXPECTED_DEPENDENCY_GRAPH_BLOB:
        fail("dependency-graph migration blob identity differs")
    if lock.get("vendored_dependency_graph_migration") != VENDORED_DEPENDENCY_GRAPH_MIGRATION:
        fail("dependency-graph vendored migration path differs")
    if lock.get("visibility_immutability_revision") != EXPECTED_VISIBILITY_REVISION:
        fail("visibility migration revision differs")
    if lock.get("visibility_immutability_migration") != VISIBILITY_MIGRATION:
        fail("visibility canonical migration path differs")
    if lock.get("visibility_immutability_blob_sha") != EXPECTED_VISIBILITY_BLOB:
        fail("visibility migration blob identity differs")
    if lock.get("vendored_visibility_immutability_migration") != VENDORED_VISIBILITY_MIGRATION:
        fail("visibility vendored migration path differs")
    source = ROOT / lock["vendored_copy"]
    if not source.is_file():
        fail("package-owned registry SQL is missing")
    actual_blob = git("hash-object", str(source.relative_to(ROOT)))
    if actual_blob != EXPECTED_REGISTRY_BLOB:
        fail(f"package-owned registry SQL blob differs: {actual_blob}")

    graph_patch = ROOT / lock["vendored_dependency_graph_migration"]
    if not graph_patch.is_file():
        fail("package-owned dependency-graph migration is missing")
    actual_graph_blob = git("hash-object", str(graph_patch.relative_to(ROOT)))
    if actual_graph_blob != EXPECTED_DEPENDENCY_GRAPH_BLOB:
        fail(f"vendored dependency-graph migration blob differs: {actual_graph_blob}")
    graph_text = graph_patch.read_text(encoding="utf-8").lower()
    for required_fragment in (
        "create table if not exists zed_dependency_graph_artifacts",
        "create table if not exists zed_dependency_graph_edges",
        "zed_dependency_graph_artifacts_document_binding_chk",
        "zed_dependency_graph_artifacts_immutable",
        "zed_dependency_graph_edges_immutable",
        "must be inserted unsealed",
        "zd004",
        "zd005",
    ):
        if required_fragment not in graph_text:
            fail(f"dependency-graph migration is missing {required_fragment}")
    if "public package % cannot become non-public" in graph_text:
        fail("dependency-graph migration mixes visibility policy")

    patch = ROOT / lock["vendored_visibility_immutability_migration"]
    if not patch.is_file():
        fail("package-owned visibility migration is missing")
    actual_patch_blob = git("hash-object", str(patch.relative_to(ROOT)))
    if actual_patch_blob != EXPECTED_VISIBILITY_BLOB:
        fail(f"vendored visibility migration blob differs: {actual_patch_blob}")
    patch_text = patch.read_text(encoding="utf-8").lower()
    for required_fragment in ("create or replace function", "zd003"):
        if required_fragment not in patch_text:
            fail(f"visibility migration is missing {required_fragment}")
    for forbidden_fragment in ("\ncreate trigger", "\nalter table", "\ncreate table"):
        if forbidden_fragment in patch_text:
            fail(f"visibility migration replays base DDL: {forbidden_fragment}")
    text = source.read_text(encoding="utf-8").lower()
    required = {
        "zed_users",
        "zed_orgs",
        "zed_projects",
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
    }
    missing = sorted(table for table in required if f"create table if not exists {table}" not in text)
    if missing:
        fail(f"package-owned registry SQL is missing tables: {missing}")
    for required_fragment in (
        "zd001",
        "zd002",
        "zd003",
        "public package % cannot become non-public",
        "zed_packages_visibility_guard",
    ):
        if required_fragment not in text:
            fail(f"registry policy is missing {required_fragment}")

    schema = (ROOT / "src/rust-orm/schema.rs").read_text(encoding="utf-8")
    for name, expected in (
        ("SCHEMA_REPOSITORY", EXPECTED_PACKAGE),
        ("SCHEMA_PACKAGE", EXPECTED_SCHEMA_PACKAGE),
        ("SCHEMA_PACKAGE_MANIFEST", EXPECTED_SCHEMA_MANIFEST),
        ("REGISTRY_DDL_PATH", "src/rust-orm/sql/registry.sql"),
        ("REGISTRY_DDL_BLOB_SHA", EXPECTED_REGISTRY_BLOB),
        ("DEPENDENCY_GRAPH_MIGRATION_IDENTITY_SUFFIX", EXPECTED_DEPENDENCY_GRAPH_REVISION),
        ("DEPENDENCY_GRAPH_MIGRATION_PATH", VENDORED_DEPENDENCY_GRAPH_MIGRATION),
        (
            "DEPENDENCY_GRAPH_MIGRATION_BLOB_SHA",
            EXPECTED_DEPENDENCY_GRAPH_BLOB,
        ),
        ("VISIBILITY_IMMUTABILITY_MIGRATION_IDENTITY_SUFFIX", EXPECTED_VISIBILITY_REVISION),
        ("VISIBILITY_IMMUTABILITY_MIGRATION_PATH", VENDORED_VISIBILITY_MIGRATION),
        (
            "VISIBILITY_IMMUTABILITY_MIGRATION_BLOB_SHA",
            EXPECTED_VISIBILITY_BLOB,
        ),
    ):
        if rust_string_const(schema, name) != expected:
            fail(f"Rust schema constant {name} differs from its locked migration")

    migrations = (ROOT / "src/rust-orm/migrations.rs").read_text(encoding="utf-8")
    if "registry@c8bdc06d74746acc6439f9527ebd02697fdf028b" not in migrations:
        fail("historical base ledger identity changed")
    if "Self::HistoricalBase,\n        Self::DependencyGraph,\n        Self::VisibilityImmutability" not in migrations:
        fail("ordered migration ledger differs")
    for vendored in (
        VENDORED_DEPENDENCY_GRAPH_MIGRATION.removeprefix("src/rust-orm/"),
        VENDORED_VISIBILITY_MIGRATION.removeprefix("src/rust-orm/"),
    ):
        if f'include_str!("{vendored}")' not in migrations:
            fail(f"migration runner does not include {vendored}")

    manifest = tomllib.loads((ROOT / ".zpkg.toml").read_text(encoding="utf-8"))
    if "oresoftware/k8s-libs-and-shared-defs" in manifest.get("dependencies", {}):
        fail("product package still depends on shared definitions")


def assert_routes() -> None:
    contract = read_json("contracts/api-routes.v1.json")
    if contract.get("schemaVersion") != 1:
        fail("route contract version differs")
    prefixes = contract.get("prefixes")
    if prefixes != {
        "internalApi": "/v1",
        "edgeApi": "/api/v1",
        "registry": "/api/v1/registry",
    }:
        fail(f"route prefixes differ: {prefixes!r}")
    registry_routes = contract.get("routes", {}).get("registry", [])
    if not registry_routes or any("/api/v1/registry/" not in route for route in registry_routes):
        fail("every registry route must remain under /api/v1/registry")
    storage = contract.get("artifactStorage", {})
    if storage.get("backend") != "cloudflare-r2":
        fail("artifact backend must be Cloudflare R2")
    if storage.get("keyTemplate") != "zed/v1/packages/{org}/{package}/{version}/{sha256}.{extension}":
        fail("R2 key template differs")
    download = storage.get("download", {})
    expected_download = {
        "apiAuthorizes": True,
        "ledgerBeforeRedirect": True,
        "status": 307,
        "presignedR2Url": True,
        "apiProxiesArtifactBytes": False,
        "webProxiesArtifactBytes": False,
    }
    if download != expected_download:
        fail(f"download boundary differs: {download!r}")
    pages = set(contract.get("webPages", []))
    required_pages = {
        "GET /",
        "GET /search",
        "GET /packages",
        "GET /dashboard",
        "GET /publish",
        "GET /developers",
        "GET /settings",
    }
    if not required_pages <= pages:
        fail(f"web page contract is incomplete: {sorted(required_pages - pages)}")


def assert_no_duplicate_orm() -> None:
    for forbidden in (ROOT / "src/rust-orm-core", ROOT / "src/zed-orm"):
        if forbidden.exists():
            fail(f"parallel ORM authority reappeared: {forbidden}")
    cargo = tomllib.loads((ROOT / "src/rust-orm/Cargo.toml").read_text(encoding="utf-8"))
    if cargo["package"]["name"] != "zed-orm-core":
        fail("compatibility ORM crate identity changed")
    if "read-write" not in cargo.get("features", {}):
        fail("write surface is no longer feature-gated")


def main() -> None:
    assert_history()
    assert_package()
    assert_schema_ownership()
    assert_routes()
    assert_no_duplicate_orm()
    summary = {
        "package": EXPECTED_PACKAGE,
        "schemaPackage": EXPECTED_SCHEMA_PACKAGE,
        "schemaAuthority": EXPECTED_SCHEMA_AUTHORITY,
        "merge": MERGE,
        "semanticFold": SEMANTIC_FOLD,
        "historicalSharedDefsRevision": EXPECTED_SHARED_DEFS_REVISION,
        "registryBlob": EXPECTED_REGISTRY_BLOB,
        "dependencyGraphMigrationRevision": EXPECTED_DEPENDENCY_GRAPH_REVISION,
        "dependencyGraphMigrationBlob": EXPECTED_DEPENDENCY_GRAPH_BLOB,
        "visibilityMigrationRevision": EXPECTED_VISIBILITY_REVISION,
        "visibilityMigrationBlob": EXPECTED_VISIBILITY_BLOB,
        "routeContract": 1,
    }
    print(json.dumps(summary, sort_keys=True))


if __name__ == "__main__":
    main()
