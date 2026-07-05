#!/usr/bin/env python3
"""Verify Cargo feature docs stay in sync with Cargo.toml."""

from __future__ import annotations

import re
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DOCS = [
    ROOT / "docs" / "quick-start.md",
    ROOT / "README.md",
]

TABLE_HEADERS = {
    ROOT / "docs" / "quick-start.md": "Feature",
    ROOT / "README.md": "Feature",
}


def cargo_features() -> list[str]:
    with (ROOT / "Cargo.toml").open("rb") as cargo:
        features = tomllib.load(cargo)["features"]
    return sorted(feature for feature in features if feature != "default")


def documented_features(path: Path) -> set[str]:
    row_re = re.compile(r"^\|\s*`([^`]+)`\s*\|")
    header_re = re.compile(rf"^\|\s*{re.escape(TABLE_HEADERS[path])}\s*\|")
    documented: set[str] = set()
    in_feature_table = False
    for line in path.read_text(encoding="utf-8").splitlines():
        if header_re.match(line):
            in_feature_table = True
            continue
        if in_feature_table and not line.startswith("|"):
            break
        if not in_feature_table:
            continue
        match = row_re.match(line)
        if match:
            documented.add(match.group(1))
    return documented


def main() -> int:
    features = set(cargo_features())
    failed = False

    for path in DOCS:
        documented = documented_features(path)
        missing = sorted(features - documented)
        extra = sorted(documented - features)
        if missing or extra:
            failed = True
            rel = path.relative_to(ROOT)
            if missing:
                print(
                    f"{rel}: missing feature table rows: {', '.join(missing)}",
                    file=sys.stderr,
                )
            if extra:
                print(
                    f"{rel}: feature table rows not in Cargo.toml: {', '.join(extra)}",
                    file=sys.stderr,
                )

    if failed:
        return 1

    print(f"feature tables include {len(features)} Cargo features")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
