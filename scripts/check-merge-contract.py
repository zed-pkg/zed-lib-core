#!/usr/bin/env python3
"""Fail-closed certification for the zed-lib-core semantic merge."""

from __future__ import annotations

import hashlib
import json
import pathlib
import re
import subprocess
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[1]
MERGE = "f27f72cc65640407409d38953c8d30ee4c95f3a6"
SEMANTIC_FOLD = "9fdc5fed96b707b99b3b02e6541060831c3d70fd"
PARENTS = (
    "430aafe24b6c3ab1263f1351ab4941545f592f19",
    "a5dabf3685db94ffdf5ae30cb3b3e4cc1cce298f",
)
EXPECTED_SHARED_DEFS_REVISION = "d8fb884023a26de79d4f5d533f486a2d3dbec7cc"
EXPECTED_REGISTRY_BLOB = "3a8ee3f9cba22d7ec2c66e93448ab96e9c79afcf"
EXPECTED_PACKAGE = "zed-pkg/zed-lib-core"


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
    required_targets = {"whole", "rust", "rust-orm", "conformance", "dart", "typescript"}
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


def assert_shared_defs() -> None:
    lock = read_json("shared-defs.lock.json")
    if lock.get("revision") != EXPECTED_SHARED_DEFS_REVISION:
        fail("shared-definitions revision differs")
    if lock.get("registry_blob_sha") != EXPECTED_REGISTRY_BLOB:
        fail("registry blob identity differs")
    source = ROOT / lock["vendored_copy"]
    if not source.is_file():
        fail("vendored registry SQL is missing")
    actual_blob = git("hash-object", str(source.relative_to(ROOT)))
    if actual_blob != EXPECTED_REGISTRY_BLOB:
        fail(f"vendored registry SQL blob differs: {actual_blob}")
    text = source.read_text(encoding="utf-8")
    required = {
        "zed_users",
        "zed_orgs",
        "zed_projects",
        "zed_packages",
        "zed_package_versions",
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
    for required_fragment in ("ZD001", "ZD002", "zed_packages_visibility_guard"):
        if required_fragment not in text:
            fail(f"registry policy is missing {required_fragment}")


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
        "routeContract": 1,
    }
    print(json.dumps(summary, sort_keys=True))


if __name__ == "__main__":
    main()
