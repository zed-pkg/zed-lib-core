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
EXPECTED_SHARED_DEFS_REVISION = "d58ec90c0129151d1c09d2cf59b2804087059ef5"
EXPECTED_REGISTRY_BLOB = "eb80355d09f0c2d4c468dc46aa6ddbd5b06993e9"
EXPECTED_DEPENDENCY_GRAPH_REVISION = "d9d33e14bead8c385aa4500fe33b56922ac63550"
EXPECTED_DEPENDENCY_GRAPH_BLOB = "f17fd7d28a808f5fd8d26e92f4af3f0429d2cda1"
EXPECTED_VISIBILITY_REVISION = "d54c3485ee7f0b7e0f816c42b274d1bc563a0d7c"
EXPECTED_VISIBILITY_BLOB = "8612f037dce7de6d7db66ee96db7996b33b32ea9"
EXPECTED_PACKAGE = "zed-pkg/zed-lib-core"

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
        "rust-orm",
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


def assert_shared_defs() -> None:
    lock = read_json("shared-defs.lock.json")
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
        fail("vendored registry SQL is missing")
    actual_blob = git("hash-object", str(source.relative_to(ROOT)))
    if actual_blob != EXPECTED_REGISTRY_BLOB:
        fail(f"vendored registry SQL blob differs: {actual_blob}")

    graph_patch = ROOT / lock["vendored_dependency_graph_migration"]
    if not graph_patch.is_file():
        fail("vendored dependency-graph migration is missing")
    actual_graph_blob = git("hash-object", str(graph_patch.relative_to(ROOT)))
    if actual_graph_blob != EXPECTED_DEPENDENCY_GRAPH_BLOB:
        fail(f"vendored dependency-graph migration blob differs: {actual_graph_blob}")
    graph_text = graph_patch.read_text(encoding="utf-8").lower()
    for required_fragment in (
        "create table if not exists zed_dependency_graph_artifacts",
        "create table if not exists zed_dependency_graph_edges",
        "do $zed_graph_constraints$",
        "zed_dependency_graph_edges_incoming_idx",
        "zed_dependency_graph_edges_unresolved_target_idx",
    ):
        if required_fragment not in graph_text:
            fail(f"dependency-graph migration is missing {required_fragment}")
    for forbidden_fragment in ("\ncreate trigger", "\ndrop table", "zd003"):
        if forbidden_fragment in graph_text:
            fail(f"dependency-graph migration mixes unrelated DDL: {forbidden_fragment}")

    patch = ROOT / lock["vendored_visibility_immutability_migration"]
    if not patch.is_file():
        fail("vendored visibility migration is missing")
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
        fail(f"vendored registry SQL is missing tables: {missing}")
    for required_fragment in ("zd001", "zd002", "zed_packages_visibility_guard"):
        if required_fragment not in text:
            fail(f"registry policy is missing {required_fragment}")

    schema = (ROOT / "src/rust-orm/schema.rs").read_text(encoding="utf-8")
    for name, expected in (
        ("SHARED_DEFS_DEPENDENCY_GRAPH_REVISION", EXPECTED_DEPENDENCY_GRAPH_REVISION),
        ("SHARED_DEFS_DEPENDENCY_GRAPH_MIGRATION", DEPENDENCY_GRAPH_MIGRATION),
        (
            "SHARED_DEFS_DEPENDENCY_GRAPH_MIGRATION_BLOB_SHA",
            EXPECTED_DEPENDENCY_GRAPH_BLOB,
        ),
        ("SHARED_DEFS_VISIBILITY_IMMUTABILITY_REVISION", EXPECTED_VISIBILITY_REVISION),
        ("SHARED_DEFS_VISIBILITY_IMMUTABILITY_MIGRATION", VISIBILITY_MIGRATION),
        (
            "SHARED_DEFS_VISIBILITY_IMMUTABILITY_MIGRATION_BLOB_SHA",
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
    assert_shared_defs()
    assert_routes()
    assert_no_duplicate_orm()
    summary = {
        "package": EXPECTED_PACKAGE,
        "merge": MERGE,
        "semanticFold": SEMANTIC_FOLD,
        "sharedDefsRevision": EXPECTED_SHARED_DEFS_REVISION,
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
