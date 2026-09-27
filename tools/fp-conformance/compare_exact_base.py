#!/usr/bin/env python3
"""Compare FP-conformance counts for an exact PR base and exact contributor head."""

from __future__ import annotations

import argparse
import json
import pathlib


def counts(path: pathlib.Path) -> dict[str, int]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    raw = payload.get("counts")
    if not isinstance(raw, dict):
        raise SystemExit(f"{path}: missing counts object")
    result: dict[str, int] = {}
    for code, value in raw.items():
        if not isinstance(code, str) or not isinstance(value, int) or value < 0:
            raise SystemExit(f"{path}: invalid count entry {code!r}={value!r}")
        result[code] = value
    return result


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base", required=True, type=pathlib.Path)
    parser.add_argument("--head", required=True, type=pathlib.Path)
    args = parser.parse_args()

    base = counts(args.base)
    head = counts(args.head)
    codes = sorted(set(base) | set(head))
    regressions = [
        (code, base.get(code, 0), head.get(code, 0))
        for code in codes
        if head.get(code, 0) > base.get(code, 0)
    ]
    improvements = [
        (code, base.get(code, 0), head.get(code, 0))
        for code in codes
        if head.get(code, 0) < base.get(code, 0)
    ]

    if improvements:
        print("fp-conformance improvements:")
        for code, before, after in improvements:
            print(f"  {code}: {before} -> {after}")

    if regressions:
        print("fp-conformance: regression against exact base")
        for code, before, after in regressions:
            print(f"  {code}: {before} -> {after}")
        return 1

    print("fp-conformance: pass; no rule count increased from exact base")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
