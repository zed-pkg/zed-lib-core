#!/usr/bin/env python3
"""Fail closed when the zed-lock Cargo and root Zed target contracts drift.

zed-lock is the `src/rust-lock` slice of zed-pkg/zed-lib-core: a Cargo
workspace member published through the repository-root Zed manifest. The slice
must not declare a second `.zpkg.toml`; the root `targets.rust-lock` entry is
the sole Zed package authority for this folded crate.
"""

from __future__ import annotations

import os
import pathlib
import sys
import tomllib

DEFAULT_ROOT = pathlib.Path(__file__).resolve().parents[1]
ROOT = pathlib.Path(os.environ.get("ZED_LOCK_PACKAGE_ROOT", DEFAULT_ROOT)).resolve()
DEFAULT_REPOSITORY_ROOT = ROOT.parents[1]
REPOSITORY_ROOT = pathlib.Path(
    os.environ.get("ZED_LOCK_REPOSITORY_ROOT", DEFAULT_REPOSITORY_ROOT)
).resolve()
EXPECTED_SOURCE_COMMIT = "fd3b08eb1ac170518cb795e662318ae2714b1176"


def load_toml(path: pathlib.Path) -> dict[str, object]:
    with path.open("rb") as stream:
        return tomllib.load(stream)


def main() -> int:
    errors: list[str] = []
    cargo_path = ROOT / "Cargo.toml"
    root_zpkg_path = REPOSITORY_ROOT / ".zpkg.toml"
    nested_zpkg_path = ROOT / ".zpkg.toml"
    zed_env_path = ROOT / "zed-env.toml"
    provenance_path = ROOT / "PROVENANCE.md"

    for path in (
        cargo_path,
        root_zpkg_path,
        zed_env_path,
        provenance_path,
        ROOT / "lib.rs",
    ):
        if not path.is_file():
            try:
                display = path.relative_to(REPOSITORY_ROOT)
            except ValueError:
                display = path
            errors.append(f"missing required package file: {display}")

    if nested_zpkg_path.exists():
        errors.append(
            "src/rust-lock/.zpkg.toml must not exist: the repository-root "
            "targets.rust-lock entry is the sole Zed package authority"
        )

    if errors:
        return report(errors)

    cargo = load_toml(cargo_path)
    root_zpkg = load_toml(root_zpkg_path)
    zed_env = load_toml(zed_env_path)
    cargo_package = cargo.get("package", {})

    expected_cargo_fields = {
        "name": "zed-lock",
        "version": "0.1.1",
        "license": "MIT",
    }
    for field, expected in expected_cargo_fields.items():
        cargo_value = cargo_package.get(field)
        if cargo_value != expected:
            errors.append(
                f"Cargo package.{field} must be {expected!r}, got {cargo_value!r}"
            )

    if cargo_package.get("rust-version") != "1.88":
        errors.append(
            "Cargo package.rust-version must be '1.88', the first supported compiler for the extracted let-chain implementation"
        )
    if cargo_package.get("repository") != "https://github.com/zed-pkg/zed-lib-core":
        errors.append("Cargo package.repository must point at zed-pkg/zed-lib-core")
    lib_section = cargo.get("lib", {})
    if lib_section.get("path") != "lib.rs" or lib_section.get("name") != "zed_lock":
        errors.append(
            "Cargo [lib] must be name = 'zed_lock', path = 'lib.rs': the crate root sits "
            "beside the manifest like every other zed-lib-core slice"
        )

    root_package = root_zpkg.get("package", {})
    if root_package.get("org") != "zed-pkg" or root_package.get("name") != "zed-lib-core":
        errors.append("repository-root Zed package must be zed-pkg/zed-lib-core")
    if root_package.get("version") != "0.1.0":
        errors.append("repository-root Zed package version must remain 0.1.0")
    repository = root_package.get("repository", {})
    if repository.get("vcs") != "git":
        errors.append("repository-root package.repository.vcs must be 'git'")
    if repository.get("url") != "https://github.com/zed-pkg/zed-lib-core":
        errors.append("repository-root package.repository.url must point at zed-pkg/zed-lib-core")

    targets = root_zpkg.get("targets", {})
    lock_target = targets.get("rust-lock") if isinstance(targets, dict) else None
    expected_lock_target = {
        "dir": "src/rust-lock",
        "name": "zed-lock",
        "adapter": "rust",
    }
    if lock_target != expected_lock_target:
        errors.append(
            f"repository-root targets.rust-lock must be {expected_lock_target!r}, got {lock_target!r}"
        )

    if zed_env.get("schema") != 2:
        errors.append("zed-env.toml must declare schema = 2")
    tasks = zed_env.get("tasks", {})
    expected_tasks = {
        "package-contract": ["python3 scripts/check-package-contract.py"],
        "format": ["cargo fmt --all --check"],
        "lint": ["cargo clippy --locked --all-targets -- -D warnings"],
        "test": ["cargo test --locked --all-targets"],
    }
    if not isinstance(tasks, dict):
        errors.append("zed-env.toml [tasks] must be a table")
    else:
        if set(tasks) != set(expected_tasks):
            errors.append(
                "zed-env.toml tasks must be exactly package-contract, format, lint, and test"
            )
        for name, expected_run in expected_tasks.items():
            task = tasks.get(name)
            if not isinstance(task, dict) or task.get("run") != expected_run:
                errors.append(
                    f"zed-env.toml tasks.{name}.run must be {expected_run!r}"
                )

    placeholder_lock = ROOT / ".zpkg.lock"
    if placeholder_lock.exists():
        normalized_lock = placeholder_lock.read_text(encoding="utf-8").replace(
            "\r\n", "\n"
        ).strip()
        if normalized_lock == "version = 1":
            errors.append(
                ".zpkg.lock is an empty placeholder; omit it until the package has Zed dependencies"
            )

    provenance = provenance_path.read_text(encoding="utf-8")
    for required in (
        "source repository: `zed-pkg/zed-cli`",
        f"source commit: `{EXPECTED_SOURCE_COMMIT}`",
        "source path: `crates/zed-lock`",
        "folded into: `zed-pkg/zed-lib-core`",
        "fold path: `src/rust-lock`",
    ):
        if required not in provenance:
            errors.append(f"PROVENANCE.md is missing {required!r}")

    source = (ROOT / "lib.rs").read_text(encoding="utf-8")
    production_source = source.split("\n#[cfg(test)]", 1)[0]
    if "FileExt::lock_exclusive" not in production_source:
        errors.append("source no longer contains the kernel descriptor-lock authority")
    if "thread::sleep" in production_source:
        errors.append(
            "production source must not regress to lock polling with thread::sleep"
        )

    return report(errors)


def report(errors: list[str]) -> int:
    if errors:
        for error in errors:
            print(f"error: {error}", file=sys.stderr)
        return 1
    print(
        "zed-lock Cargo, root Zed target, schema-2 task, and extraction provenance contracts are consistent"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
