#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import json
import pathlib
import sys
import tempfile
import unittest

MODULE_PATH = pathlib.Path(__file__).with_name("compare_exact_base.py")
SPEC = importlib.util.spec_from_file_location("compare_exact_base", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


class CompareExactBaseTest(unittest.TestCase):
    def write(self, counts: dict[str, int]) -> pathlib.Path:
        handle = tempfile.NamedTemporaryFile("w", encoding="utf-8", delete=False)
        with handle:
            json.dump({"counts": counts}, handle)
        return pathlib.Path(handle.name)

    def test_missing_rule_is_zero(self) -> None:
        base = MODULE.counts(self.write({"RS001": 4}))
        head = MODULE.counts(self.write({"RS001": 4, "RS002": 1}))
        self.assertEqual(base.get("RS002", 0), 0)
        self.assertEqual(head["RS002"], 1)

    def test_counts_reject_negative_values(self) -> None:
        with self.assertRaises(SystemExit):
            MODULE.counts(self.write({"RS001": -1}))

    def test_count_reader_preserves_exact_values(self) -> None:
        payload = {"RS001": 111, "RS003": 91, "XX001": 7}
        self.assertEqual(MODULE.counts(self.write(payload)), payload)


if __name__ == "__main__":
    unittest.main()
