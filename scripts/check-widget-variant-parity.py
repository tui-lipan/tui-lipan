#!/usr/bin/env python3
"""Check widget variant plumbing against src/widget_manifest.rs.

This is intentionally textual rather than compiler-driven so feature-gated
variants are checked regardless of the current cargo feature set.
"""

from __future__ import annotations

import re
import sys
from dataclasses import dataclass
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]

MANIFEST = ROOT / "src/widget_manifest.rs"
ELEMENT = ROOT / "src/core/element.rs"
NODE_KIND = ROOT / "src/core/node/kind.rs"
RENDER = ROOT / "src/backend/ratatui_backend/render/mod.rs"

ELEMENT_ONLY_CATEGORY = "element_only_const_auto"


@dataclass(frozen=True)
class Manifest:
    all_variants: set[str]
    element_only: set[str]
    categories: dict[str, set[str]]


def read(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except OSError as exc:
        raise SystemExit(f"error: failed to read {path.relative_to(ROOT)}: {exc}") from exc


def strip_comments(text: str) -> str:
    text = re.sub(r"//.*", "", text)
    return re.sub(r"/\*.*?\*/", "", text, flags=re.DOTALL)


def find_balanced_block(text: str, open_brace_index: int) -> str:
    depth = 0
    for index in range(open_brace_index, len(text)):
        char = text[index]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return text[open_brace_index + 1 : index]
    raise ValueError("unclosed balanced block")


def enum_body(text: str, enum_name: str, path: Path) -> str:
    match = re.search(rf"\benum\s+{re.escape(enum_name)}\s*{{", text)
    if not match:
        raise SystemExit(f"error: {path.relative_to(ROOT)}: enum {enum_name} not found")
    return find_balanced_block(text, match.end() - 1)


def parse_enum_variants(path: Path, enum_name: str) -> set[str]:
    body = strip_comments(enum_body(read(path), enum_name, path))
    return set(re.findall(r"(?m)^\s*([A-Z][A-Za-z0-9_]*)\s*\(", body))


def parse_manifest() -> Manifest:
    text = strip_comments(read(MANIFEST))
    categories: dict[str, set[str]] = {}
    for category, body in re.findall(r"@(\w+)\s*\[(.*?)\]", text, flags=re.DOTALL):
        variants = set(re.findall(r"\b([A-Z][A-Za-z0-9_]*)\b\s*(?:=>|,)", body))
        categories[category] = variants

    all_variants = set().union(*categories.values()) if categories else set()
    element_only = categories.get(ELEMENT_ONLY_CATEGORY, set())
    return Manifest(all_variants=all_variants, element_only=element_only, categories=categories)


def parse_delegate_arms() -> set[str]:
    text = strip_comments(read(NODE_KIND))
    match = re.search(r"macro_rules!\s+node_kind_delegate_match\s*{", text)
    if not match:
        raise SystemExit(
            f"error: {NODE_KIND.relative_to(ROOT)}: node_kind_delegate_match macro not found"
        )
    body = find_balanced_block(text, match.end() - 1)
    return set(re.findall(r"\bSelf::([A-Z][A-Za-z0-9_]*)\s*\(", body))


def parse_render_node_arms() -> set[str]:
    text = strip_comments(read(RENDER))
    fn_match = re.search(r"\bfn\s+render_node\s*\(", text)
    if not fn_match:
        raise SystemExit(f"error: {RENDER.relative_to(ROOT)}: render_node function not found")

    fn_open = text.find("{", fn_match.end())
    if fn_open == -1:
        raise SystemExit(f"error: {RENDER.relative_to(ROOT)}: render_node body not found")
    fn_body = find_balanced_block(text, fn_open)

    match_start = fn_body.find("match &node.kind")
    if match_start == -1:
        raise SystemExit(f"error: {RENDER.relative_to(ROOT)}: render_node NodeKind match not found")
    match_open = fn_body.find("{", match_start)
    if match_open == -1:
        raise SystemExit(f"error: {RENDER.relative_to(ROOT)}: render_node match body not found")
    match_body = find_balanced_block(fn_body, match_open)
    return set(re.findall(r"\bNodeKind::([A-Z][A-Za-z0-9_]*)\s*\(", match_body))


def format_set(values: set[str]) -> str:
    if not values:
        return "(none)"
    return ", ".join(sorted(values))


def check(label: str, path: Path, expected: set[str], actual: set[str], diagnostics: list[str]) -> None:
    missing = expected - actual
    extra = actual - expected
    if not missing and not extra:
        return

    details = [f"{path.relative_to(ROOT)}: {label} is out of sync with widget_manifest.rs"]
    if missing:
        details.append(f"  missing from {label}: {format_set(missing)}")
    if extra:
        details.append(f"  extra in {label}: {format_set(extra)}")
    diagnostics.append("\n".join(details))


def main() -> int:
    manifest = parse_manifest()
    if not manifest.all_variants:
        print(f"error: {MANIFEST.relative_to(ROOT)}: no manifest variants found", file=sys.stderr)
        return 2

    expected_node_variants = manifest.all_variants - manifest.element_only

    diagnostics: list[str] = []
    check(
        "ElementKind variants",
        ELEMENT,
        manifest.all_variants,
        parse_enum_variants(ELEMENT, "ElementKind"),
        diagnostics,
    )
    check(
        "NodeKind variants",
        NODE_KIND,
        expected_node_variants,
        parse_enum_variants(NODE_KIND, "NodeKind"),
        diagnostics,
    )
    check(
        "node_kind_delegate_match arms",
        NODE_KIND,
        expected_node_variants,
        parse_delegate_arms(),
        diagnostics,
    )
    check(
        "render_node NodeKind arms",
        RENDER,
        expected_node_variants,
        parse_render_node_arms(),
        diagnostics,
    )

    if diagnostics:
        print("Widget variant parity check failed:\n", file=sys.stderr)
        print("\n\n".join(diagnostics), file=sys.stderr)
        print(
            "\nUpdate src/widget_manifest.rs and all manual variant plumbing together. "
            "Run scripts/generate-node-kind-delegate-arms.py --write if generated "
            "delegate arms changed. Feature-gated variants are checked textually.",
            file=sys.stderr,
        )
        return 1

    print(
        "Widget variant parity OK "
        f"({len(manifest.all_variants)} ElementKind variants, "
        f"{len(expected_node_variants)} NodeKind variants checked)."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
