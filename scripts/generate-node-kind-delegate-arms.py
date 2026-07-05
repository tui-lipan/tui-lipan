#!/usr/bin/env python3
"""Generate/check NodeKind WidgetNode delegation arms from widget_manifest.rs."""

from __future__ import annotations

import argparse
import difflib
import re
import sys
from dataclasses import dataclass
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "src/widget_manifest.rs"
NODE_KIND = ROOT / "src/core/node/kind.rs"

ELEMENT_ONLY_CATEGORY = "element_only_const_auto"
START = "            // BEGIN GENERATED: node_kind_delegate_match arms"
END = "            // END GENERATED: node_kind_delegate_match arms"
FIX_COMMAND = "python3 scripts/generate-node-kind-delegate-arms.py --write"


@dataclass(frozen=True)
class Variant:
    name: str
    category: str
    feature: str | None = None


def read(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except OSError as exc:
        raise SystemExit(f"error: failed to read {path.relative_to(ROOT)}: {exc}") from exc


def write(path: Path, text: str) -> None:
    try:
        path.write_text(text, encoding="utf-8")
    except OSError as exc:
        raise SystemExit(f"error: failed to write {path.relative_to(ROOT)}: {exc}") from exc


def strip_comments(text: str) -> str:
    text = re.sub(r"//.*", "", text)
    return re.sub(r"/\*.*?\*/", "", text, flags=re.DOTALL)


def parse_manifest() -> list[Variant]:
    text = strip_comments(read(MANIFEST))
    variants: list[Variant] = []
    seen: set[str] = set()

    for category, body in re.findall(r"@(\w+)\s*\[(.*?)\]", text, flags=re.DOTALL):
        position = 0
        entry_pattern = re.compile(
            r"\s*([A-Z][A-Za-z0-9_]*)\s*(?:=>\s*\"([^\"]+)\"\s*)?,",
            flags=re.DOTALL,
        )
        for match in entry_pattern.finditer(body):
            gap = body[position : match.start()].strip()
            if gap:
                raise SystemExit(
                    f"error: {MANIFEST.relative_to(ROOT)}: could not parse manifest entry "
                    f"in @{category}: {gap!r}"
                )
            name, feature = match.groups()
            category_is_gated = category.endswith("_gated")
            if category_is_gated and feature is None:
                raise SystemExit(
                    f"error: {MANIFEST.relative_to(ROOT)}: @{category} variant {name} "
                    "must specify a feature with `=> \"feature\"`"
                )
            if not category_is_gated and feature is not None:
                raise SystemExit(
                    f"error: {MANIFEST.relative_to(ROOT)}: @{category} variant {name} "
                    "must not specify a feature"
                )
            if name in seen:
                raise SystemExit(
                    f"error: {MANIFEST.relative_to(ROOT)}: duplicate manifest variant {name}"
                )
            seen.add(name)
            variants.append(Variant(name=name, category=category, feature=feature))
            position = match.end()

        trailing = body[position:].strip()
        if trailing:
            raise SystemExit(
                f"error: {MANIFEST.relative_to(ROOT)}: could not parse manifest entry "
                f"in @{category}: {trailing!r}"
            )

    if not variants:
        raise SystemExit(f"error: {MANIFEST.relative_to(ROOT)}: no manifest variants found")

    return variants


def render_delegate_arms(variants: list[Variant]) -> str:
    lines: list[str] = []
    for variant in variants:
        if variant.category == ELEMENT_ONLY_CATEGORY:
            continue
        if variant.feature:
            lines.append(f'            #[cfg(feature = "{variant.feature}")]')
        lines.append(f"            Self::{variant.name}(n) => n.$method($($arg),*),")
    return "\n".join(lines) + "\n"


def expected_block() -> str:
    return f"{START}\n{render_delegate_arms(parse_manifest())}{END}"


def replace_generated_block(text: str, replacement: str) -> tuple[str, str]:
    start_count = text.count(START)
    end_count = text.count(END)
    if start_count != 1 or end_count != 1:
        raise SystemExit(
            f"error: {NODE_KIND.relative_to(ROOT)}: expected exactly one generated block "
            f"({start_count} start markers, {end_count} end markers found)"
        )

    start_index = text.index(START)
    end_index = text.index(END, start_index)
    current = text[start_index : end_index + len(END)]
    updated = text[:start_index] + replacement + text[end_index + len(END) :]
    return updated, current


def check() -> int:
    replacement = expected_block()
    _, current = replace_generated_block(read(NODE_KIND), replacement)
    if current == replacement:
        print("NodeKind delegate arms are up to date.")
        return 0

    diff = "\n".join(
        difflib.unified_diff(
            current.splitlines(),
            replacement.splitlines(),
            fromfile=f"current:{NODE_KIND.relative_to(ROOT)}",
            tofile="expected:widget_manifest.rs",
            lineterm="",
        )
    )
    print(
        f"error: {NODE_KIND.relative_to(ROOT)} generated NodeKind delegate arms are stale.\n"
        f"Run `{FIX_COMMAND}`.\n\n{diff}",
        file=sys.stderr,
    )
    return 1


def write_generated() -> int:
    replacement = expected_block()
    original = read(NODE_KIND)
    updated, current = replace_generated_block(original, replacement)
    if current == replacement:
        print("NodeKind delegate arms are already up to date.")
        return 0

    write(NODE_KIND, updated)
    print(f"Updated {NODE_KIND.relative_to(ROOT)} generated NodeKind delegate arms.")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--write",
        action="store_true",
        help="rewrite the generated delegate-arm block instead of checking it",
    )
    args = parser.parse_args()

    if args.write:
        return write_generated()
    return check()


if __name__ == "__main__":
    raise SystemExit(main())
