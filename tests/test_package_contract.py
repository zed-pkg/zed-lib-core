#!/usr/bin/env python3
from __future__ import annotations

import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest

REPOSITORY = pathlib.Path(__file__).resolve().parents[1]
CHECKER = REPOSITORY / "scripts/check-package-contract.py"


class PackageContractTests(unittest.TestCase):
    def fixture(self) -> pathlib.Path:
        temporary = pathlib.Path(self.addCleanupTempDir())
        for name in ("Cargo.toml", ".zpkg.toml", "PROVENANCE.md"):
            shutil.copy2(REPOSITORY / name, temporary / name)
        shutil.copytree(REPOSITORY / "src", temporary / "src")
        return temporary

    def addCleanupTempDir(self) -> str:
        directory = tempfile.mkdtemp(prefix="zed-lock-package-contract-")
        self.addCleanup(shutil.rmtree, directory, ignore_errors=True)
        return directory

    def run_checker(self, root: pathlib.Path) -> subprocess.CompletedProcess[str]:
        environment = os.environ.copy()
        environment["ZED_LOCK_PACKAGE_ROOT"] = str(root)
        return subprocess.run(
            [sys.executable, str(CHECKER)],
            check=False,
            capture_output=True,
            text=True,
            env=environment,
        )

    def test_current_repository_is_consistent(self) -> None:
        result = self.run_checker(REPOSITORY)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("contracts are consistent", result.stdout)

    def test_invalid_native_registry_is_rejected(self) -> None:
        fixture = self.fixture()
        manifest = fixture / ".zpkg.toml"
        manifest.write_text(
            manifest.read_text(encoding="utf-8").replace(
                'registry = "crates-io"', 'registry = "crates.io"'
            ),
            encoding="utf-8",
        )
        result = self.run_checker(fixture)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("native.registry", result.stderr)

    def test_empty_zed_lock_placeholder_is_rejected(self) -> None:
        fixture = self.fixture()
        (fixture / ".zpkg.lock").write_text("version = 1\n", encoding="utf-8")
        result = self.run_checker(fixture)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("empty placeholder", result.stderr)

    def test_polling_regression_is_rejected(self) -> None:
        fixture = self.fixture()
        source = fixture / "src/lib.rs"
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
        result = self.run_checker(fixture)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("production source", result.stderr)


if __name__ == "__main__":
    unittest.main()
