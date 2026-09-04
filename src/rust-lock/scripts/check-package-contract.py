#!/usr/bin/env python3
"""Fail closed when the zed-lock Cargo and Zed package contracts drift.

zed-lock is the `src/rust-lock` slice of zed-pkg/zed-lib-core: a workspace
member and a nested Zed package, published under the `lock/v{version}` tag.
"""

from __future__ import annotations

import os
import pathlib
import sys
import tomllib

DEFAULT_ROOT = pathlib.Path(__file__).resolve().parents[1]
ROOT = pathlib.Path(os.environ.get("ZED_LOCK_PACKAGE_ROOT", DEFAULT_ROOT)).resolve()
EXPECTED_SOURCE_COMMIT = "fd3b08eb1ac170518cb795e662318ae2714b1176"


def load_toml(path: pathlib.Path) -> dict[str, object]:
    with path.open("rb") as stream:
        return tomllib.load(stream)


def main() -> int:
    errors: list[str] = []
    cargo_path = ROOT / "Cargo.toml"
    zpkg_path = ROOT / ".zpkg.toml"
    provenance_path = ROOT / "PROVENANCE.md"

    for path in (cargo_path, zpkg_path, provenance_path, ROOT / "lib.rs"):
        if not path.is_file():
            try:
                display = path.relative_to(ROOT)
            except ValueError:
                display = path
            errors.append(f"missing required package file: {display}")

    if errors:
        return report(errors)

    cargo = load_toml(cargo_path)
    zpkg = load_toml(zpkg_path)
    cargo_package = cargo.get("package", {})
    zpkg_package = zpkg.get("package", {})

    expected_scalar_fields = {
        "name": "zed-lock",
        "version": "0.1.1",
        "license": "MIT",
    }
    for field, expected in expected_scalar_fields.items():
        cargo_value = cargo_package.get(field)
        if cargo_value != expected:
            errors.append(
                f"Cargo package.{field} must be {expected!r}, got {cargo_value!r}"
            )
        zpkg_value = zpkg_package.get(field)
        if zpkg_value != expected:
            errors.append(
                f"Zed package.{field} must be {expected!r}, got {zpkg_value!r}"
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

    if zpkg_package.get("org") != "zed-pkg":
        errors.append("Zed package.org must be 'zed-pkg'")
    if zpkg_package.get("language") != "rust":
        errors.append("Zed package.language must be 'rust'")

    repository = zpkg_package.get("repository", {})
    if repository.get("vcs") != "git":
        errors.append("Zed package.repository.vcs must be 'git'")
    if repository.get("url") != "https://github.com/zed-pkg/zed-lib-core":
        errors.append("Zed package.repository.url must point at zed-pkg/zed-lib-core")

    publish = zpkg.get("publish", {})
    if publish.get("tag_format") != "lock/v{version}":
        errors.append(
            "Zed publish.tag_format must be 'lock/v{version}' so the nested package's "
            "tags cannot collide with the repository package or the orm slice"
        )

    # A nested slice is published by the repository package's `targets.rust-lock`
    # entry; declaring its own targets (or a crates.io native route) would make
    # two release authorities for one crate.
    if "targets" in zpkg:
        errors.append(
            "nested Zed package must not declare targets: src/rust-lock is published "
            "through zed-lib-core's root .zpkg.toml, and cargo publish remains an "
            "independent crates.io release operation"
        )
    install = zpkg.get("install", {})
    if install.get("adapter") != "rust":
        errors.append("Zed install.adapter must be 'rust'")

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
        "zed-lock Cargo, Zed package, and extraction provenance contracts are consistent"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
