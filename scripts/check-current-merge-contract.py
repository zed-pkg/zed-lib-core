#!/usr/bin/env python3
"""Certify the current zed-lib-core topology without weakening historical checks.

The historical semantic-merge checker still owns provenance, schema, route, and
ORM invariants.  This adapter replaces only its stale package-topology assertion:
`src/rust-lock` is now a distinct nested `zed-pkg/zed-lock` package authority,
not a root target of `zed-pkg/zed-lib-core`.
"""

from __future__ import annotations

import importlib.util
import json
import pathlib
import re
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[1]
LEGACY_CHECKER = ROOT / "scripts/check-merge-contract.py"

spec = importlib.util.spec_from_file_location("historical_merge_contract", LEGACY_CHECKER)
if spec is None or spec.loader is None:
    raise SystemExit("zed-lib-core merge contract: cannot load historical checker")
legacy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(legacy)


def fail(message: str) -> "NoReturn":
    legacy.fail(message)


def assert_current_package_topology() -> None:
    manifest = tomllib.loads((ROOT / ".zpkg.toml").read_text(encoding="utf-8"))
    package = manifest.get("package", {})
    identity = f"{package.get('org')}/{package.get('name')}"
    if identity != legacy.EXPECTED_PACKAGE or package.get("version") != "0.1.0":
        fail(f"unexpected package identity: {identity}@{package.get('version')}")

    # Root targets are language/conformance slices of zed-lib-core only.
    # Package authorities with their own .zpkg.toml must never also appear as
    # root targets, because that would give one directory two package owners.
    required_targets = {
        "repository",
        "rust",
        "conformance",
        "dart",
        "typescript",
    }
    targets = set(manifest.get("targets", {}))
    if targets != required_targets:
        fail(f"current root target set differs: {sorted(targets)}")
    for forbidden in ("rust-lock", "rust-orm", "sql-schema"):
        if forbidden in targets:
            fail(f"nested package authority leaked into root targets: {forbidden}")
    if manifest["targets"]["repository"].get("dir") != ".":
        fail("repository target must continue to carry the whole source tree")
    for target, target_spec in manifest["targets"].items():
        directory = ROOT / target_spec["dir"]
        if not directory.is_dir():
            fail(f"target {target} directory is missing: {directory}")

    # zed-lock remains source-folded into this repository and Cargo workspace,
    # while its Zed package/release authority is the nested manifest.
    cargo = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    workspace = cargo.get("workspace", {})
    for field in ("members", "default-members"):
        if "src/rust-lock" not in workspace.get(field, []):
            fail(f"src/rust-lock is missing from Cargo workspace {field}")

    lock_manifest_path = ROOT / "src/rust-lock/.zpkg.toml"
    if not lock_manifest_path.is_file():
        fail("nested zed-lock package manifest is missing")
    lock_manifest = tomllib.loads(lock_manifest_path.read_text(encoding="utf-8"))
    if lock_manifest.get("targets"):
        fail("nested zed-lock manifest must not declare nested targets")
    lock_package = lock_manifest.get("package", {})
    lock_identity = f"{lock_package.get('org')}/{lock_package.get('name')}"
    if lock_identity != "zed-pkg/zed-lock":
        fail(f"folded zed-lock package identity differs: {lock_identity}")
    if lock_package.get("version") != "0.1.1":
        fail(f"folded zed-lock version differs: {lock_package.get('version')}")
    if lock_package.get("language") != "rust":
        fail("folded zed-lock package language must remain rust")
    repository = lock_package.get("repository", {})
    if repository != {
        "vcs": "git",
        "url": "https://github.com/zed-pkg/zed-lib-core",
    }:
        fail(f"folded zed-lock repository authority differs: {repository!r}")
    publish = lock_manifest.get("publish", {})
    if publish.get("tag_format") != "lock/v{version}":
        fail("folded zed-lock release tag namespace differs")
    if publish.get("smoke_test") != (
        'python3 "$ZED_PKG_TEST_TARGET/scripts/check-package-contract.py" '
        '&& cargo test --locked --manifest-path "$ZED_PKG_TEST_TARGET/Cargo.toml" --all-targets'
    ):
        fail("folded zed-lock consumer smoke test differs")
    if lock_manifest.get("install") != {
        "dir": ".vendor/.zed",
        "adapter": "rust",
    }:
        fail("folded zed-lock install boundary differs")

    # The repository artifact carries the implementation source, but must not
    # carry a second copy of nested package metadata/release authority.
    root_excludes = set(manifest.get("publish", {}).get("exclude", []))
    required_lock_metadata_excludes = {
        "src/rust-lock/.zpkg.toml",
        "src/rust-lock/LICENSE",
        "src/rust-lock/README.md",
        "src/rust-lock/CHANGELOG.md",
        "src/rust-lock/SECURITY.md",
        "src/rust-lock/.github-zed-lock/**",
    }
    missing_excludes = sorted(required_lock_metadata_excludes - root_excludes)
    if missing_excludes:
        fail(f"root artifact does not exclude nested zed-lock metadata: {missing_excludes}")
    if "src/rust-lock/**" in root_excludes:
        fail("root artifact must retain folded zed-lock implementation source")

    # Preserve the historical independent ORM and SQL-schema package checks.
    orm_manifest = tomllib.loads((ROOT / legacy.EXPECTED_ORM_MANIFEST).read_text(encoding="utf-8"))
    orm_package = orm_manifest.get("package", {})
    orm_identity = f"{orm_package.get('org')}/{orm_package.get('name')}"
    if orm_identity != legacy.EXPECTED_ORM_PACKAGE:
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

    schema_manifest = tomllib.loads((ROOT / legacy.EXPECTED_SCHEMA_MANIFEST).read_text(encoding="utf-8"))
    schema_package = schema_manifest.get("package", {})
    schema_identity = f"{schema_package.get('org')}/{schema_package.get('name')}"
    if schema_identity != legacy.EXPECTED_SCHEMA_PACKAGE:
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

    # A committed Zed lock remains immutable-provenance evidence.  Lockless
    # authoring is allowed, but any lock that exists must be complete and real.
    lock_path = ROOT / ".zpkg.lock"
    if lock_path.exists():
        lock = tomllib.loads(lock_path.read_text(encoding="utf-8"))
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


def main() -> None:
    legacy.assert_history()
    assert_current_package_topology()
    legacy.assert_schema_ownership()
    legacy.assert_routes()
    legacy.assert_no_duplicate_orm()
    summary = {
        "package": legacy.EXPECTED_PACKAGE,
        "lockPackage": "zed-pkg/zed-lock@0.1.1",
        "lockAuthority": "src/rust-lock/.zpkg.toml",
        "schemaPackage": legacy.EXPECTED_SCHEMA_PACKAGE,
        "schemaAuthority": legacy.EXPECTED_SCHEMA_AUTHORITY,
        "merge": legacy.MERGE,
        "semanticFold": legacy.SEMANTIC_FOLD,
        "historicalSharedDefsRevision": legacy.EXPECTED_SHARED_DEFS_REVISION,
        "registryBlob": legacy.EXPECTED_REGISTRY_BLOB,
        "dependencyGraphMigrationRevision": legacy.EXPECTED_DEPENDENCY_GRAPH_REVISION,
        "dependencyGraphMigrationBlob": legacy.EXPECTED_DEPENDENCY_GRAPH_BLOB,
        "visibilityMigrationRevision": legacy.EXPECTED_VISIBILITY_REVISION,
        "visibilityMigrationBlob": legacy.EXPECTED_VISIBILITY_BLOB,
        "routeContract": 1,
    }
    print(json.dumps(summary, sort_keys=True))


if __name__ == "__main__":
    main()
