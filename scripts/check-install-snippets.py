#!/usr/bin/env python3
"""Verify documented install snippets stay in sync with the crate version.

Every `tui-lipan = "..."` dependency snippet in the shipped Markdown should name
the current release series. Snippets are pinned at the minor level (`"0.4"`),
so a patch release leaves them alone and only a series bump rewrites them.

Placeholder versions (`"*"`, `"..."`) and `path = ` snippets are intentional and
left untouched.

Run with `--fix` to rewrite stale snippets in place.
"""

from __future__ import annotations

import argparse
import re
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]

# Markdown that ships to users: the two crate readmes (rendered on crates.io)
# and the docs tree, which is packaged inside the published crate.
DOC_ROOTS = [
    ROOT / "README.md",
    ROOT / "tui-lipan-macro" / "README.md",
    ROOT / "docs",
]

SKIP_DIRS = {"node_modules", ".vitepress", "public"}

# `tui-lipan = "0.4"` and `tui-lipan = { version = "0.4", features = [..] }`.
SNIPPET_RE = re.compile(
    r'(?P<prefix>^tui-lipan\s*=\s*(?:\{\s*)?(?:version\s*=\s*)?)"(?P<version>[^"]+)"',
    re.MULTILINE,
)

PLACEHOLDER_RE = re.compile(r"^[*.…]+$")


def release_series() -> str:
    """The minor-level pin for the current version: 0.4.1 -> 0.4, 1.2.3 -> 1."""
    with (ROOT / "Cargo.toml").open("rb") as cargo:
        version = tomllib.load(cargo)["package"]["version"]
    major, minor, *_ = version.split(".")
    return f"{major}.{minor}" if major == "0" else major


def documents() -> list[Path]:
    paths: list[Path] = []
    for root in DOC_ROOTS:
        if root.is_file():
            paths.append(root)
            continue
        for path in sorted(root.rglob("*.md")):
            if SKIP_DIRS.isdisjoint(path.relative_to(root).parts):
                paths.append(path)
    return paths


def stale(path: Path, series: str) -> list[tuple[int, str]]:
    """Line number and version of each snippet naming a different series."""
    found = []
    text = path.read_text(encoding="utf-8")
    for match in SNIPPET_RE.finditer(text):
        version = match.group("version")
        if PLACEHOLDER_RE.match(version) or version == series:
            continue
        line = text.count("\n", 0, match.start()) + 1
        found.append((line, version))
    return found


def rewrite(path: Path, series: str) -> None:
    text = path.read_text(encoding="utf-8")

    def replace(match: re.Match[str]) -> str:
        version = match.group("version")
        if PLACEHOLDER_RE.match(version):
            return match.group(0)
        return f'{match.group("prefix")}"{series}"'

    path.write_text(SNIPPET_RE.sub(replace, text), encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--fix", action="store_true", help="rewrite stale snippets in place"
    )
    args = parser.parse_args()

    series = release_series()
    paths = documents()
    failed = False

    for path in paths:
        found = stale(path, series)
        if not found:
            continue
        rel = path.relative_to(ROOT)
        if args.fix:
            rewrite(path, series)
            for line, version in found:
                print(f"{rel}:{line}: {version} -> {series}")
            continue
        failed = True
        for line, version in found:
            print(
                f'{rel}:{line}: install snippet names "{version}", expected "{series}"',
                file=sys.stderr,
            )

    if failed:
        print(
            "\nrun `python3 scripts/check-install-snippets.py --fix` to update them",
            file=sys.stderr,
        )
        return 1

    print(f"install snippets in {len(paths)} documents name the {series} series")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
