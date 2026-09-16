#!/usr/bin/env python3
from __future__ import annotations

import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest

PACKAGE_ROOT = pathlib.Path(__file__).resolve().parents[1]
REPOSITORY_ROOT = PACKAGE_ROOT.parents[1]
CHECKER = PACKAGE_ROOT / "scripts/check-package-contract.py"


class PackageContractTests(unittest.TestCase):
    def fixture(self) -> tuple[pathlib.Path, pathlib.Path]:
        repository = pathlib.Path(self.addCleanupTempDir())
        package = repository / "src" / "rust-lock"
        package.mkdir(parents=True)
        shutil.copy2(REPOSITORY_ROOT / ".zpkg.toml", repository / ".zpkg.toml")
        for name in (
            "Cargo.toml",
            "zed-env.toml",
            "PROVENANCE.md",
            "lib.rs",
            "path_security.rs",
        ):
            shutil.copy2(PACKAGE_ROOT / name, package / name)
        return repository, package

    def addCleanupTempDir(self) -> str:
        directory = tempfile.mkdtemp(prefix="zed-lock-package-contract-")
        self.addCleanup(shutil.rmtree, directory, ignore_errors=True)
        return directory

    def run_checker(
        self, repository: pathlib.Path, package: pathlib.Path
    ) -> subprocess.CompletedProcess[str]:
        environment = os.environ.copy()
        environment["ZED_LOCK_PACKAGE_ROOT"] = str(package)
        environment["ZED_LOCK_REPOSITORY_ROOT"] = str(repository)
        return subprocess.run(
            [sys.executable, str(CHECKER)],
            check=False,
            capture_output=True,
            text=True,
            env=environment,
        )

    def test_current_repository_is_consistent(self) -> None:
        result = self.run_checker(REPOSITORY_ROOT, PACKAGE_ROOT)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("contracts are consistent", result.stdout)

    def test_nested_zpkg_authority_is_rejected(self) -> None:
        repository, package = self.fixture()
        (package / ".zpkg.toml").write_text(
            '[package]\norg = "zed-pkg"\nname = "zed-lock"\nversion = "0.1.1"\n',
            encoding="utf-8",
        )
        result = self.run_checker(repository, package)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("sole Zed package authority", result.stderr)

    def test_root_target_identity_is_enforced(self) -> None:
        repository, package = self.fixture()
        manifest = repository / ".zpkg.toml"
        manifest.write_text(
            manifest.read_text(encoding="utf-8").replace(
                'name = "zed-lock"\nadapter = "rust"',
                'name = "zed-lock-wrong"\nadapter = "rust"',
                1,
            ),
            encoding="utf-8",
        )
        result = self.run_checker(repository, package)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("targets.rust-lock", result.stderr)

    def test_root_package_identity_is_enforced(self) -> None:
        repository, package = self.fixture()
        manifest = repository / ".zpkg.toml"
        manifest.write_text(
            manifest.read_text(encoding="utf-8").replace(
                'name = "zed-lib-core"', 'name = "wrong-lib-core"', 1
            ),
            encoding="utf-8",
        )
        result = self.run_checker(repository, package)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("zed-pkg/zed-lib-core", result.stderr)

    def test_cargo_package_identity_is_enforced(self) -> None:
        repository, package = self.fixture()
        manifest = package / "Cargo.toml"
        manifest.write_text(
            manifest.read_text(encoding="utf-8").replace(
                'name = "zed-lock"', 'name = "zed-lock-wrong"', 1
            ),
            encoding="utf-8",
        )
        result = self.run_checker(repository, package)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Cargo package.name", result.stderr)

    def test_schema_two_task_plan_is_enforced(self) -> None:
        repository, package = self.fixture()
        plan = package / "zed-env.toml"
        plan.write_text(
            plan.read_text(encoding="utf-8").replace("schema = 2", "schema = 1"),
            encoding="utf-8",
        )
        result = self.run_checker(repository, package)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("schema = 2", result.stderr)

    def test_empty_zed_lock_placeholder_is_rejected(self) -> None:
        repository, package = self.fixture()
        (package / ".zpkg.lock").write_text("version = 1\n", encoding="utf-8")
        result = self.run_checker(repository, package)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("empty placeholder", result.stderr)

    def test_polling_regression_is_rejected(self) -> None:
        repository, package = self.fixture()
        source = package / "lib.rs"
        text = source.read_text(encoding="utf-8")
        marker = "\n#[cfg(test)]"
        self.assertIn(marker, text, "source fixture has no production/test boundary")
        text = text.replace(
            marker,
            "\n// regression sentinel: thread::sleep(Duration::from_millis(1));"
            + marker,
            1,
        )
        source.write_text(text, encoding="utf-8")
        result = self.run_checker(repository, package)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("production source", result.stderr)


if __name__ == "__main__":
    unittest.main()
