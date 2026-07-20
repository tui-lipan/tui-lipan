#!/usr/bin/env python3
"""Guard against `children(...)` silently discarding earlier `child(...)` calls.

Plural collection setters in this crate replace (`children`, `items`, `rows`,
`series`, ...) while their singular counterparts append (`child`, `add_series`,
...). That is a deliberate convention, but it means a builder chain like

    HStack::new()
        .child(a)
        .children([b, c])       // <- a is silently dropped

compiles cleanly and loses `a` at runtime. This exact bug shipped in
`examples/paint.rs`, where a toolbar's first three buttons never rendered.

This check walks builder chains at matching paren depth (so nested builders do
not produce false positives) and fails when a replacing setter follows an
appending one on the same chain.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SEARCH_DIRS = ("src", "examples", "tests", "benches")

# Appending setter -> the replacing setter that would discard its work.
# Only `child`/`children` has been caught in the wild so far; the rest are
# latent pairs guarded pre-emptively because they have the same shape.
APPEND_THEN_REPLACE = {
    "child": "children",
    "add_series": "series",
    "class": "classes",
    "item": "items",
    "relation": "relations",
}

CHAIN_START = re.compile(r"\b[A-Z]\w*(?:::<[^>]*>)?::(?:new|default)\s*\(\s*\)")
METHOD = re.compile(r"\.\s*(\w+)\s*\(")


def chain_methods(src: str, start: int) -> list[tuple[str, int]]:
    """Method calls at depth 0 of one builder expression beginning at `start`."""
    depth = 0
    i = start
    found: list[tuple[str, int]] = []
    while i < len(src):
        c = src[i]
        if c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
            if depth < 0:
                break
        elif c == ";" and depth == 0:
            break
        elif c == "." and depth == 0:
            m = METHOD.match(src, i)
            if m:
                found.append((m.group(1), i))
        i += 1
    return found


def scan(path: Path) -> list[str]:
    src = path.read_text()
    rel = path.relative_to(ROOT)
    problems: list[str] = []
    for anchor in CHAIN_START.finditer(src):
        calls = chain_methods(src, anchor.end())
        for append_name, replace_name in APPEND_THEN_REPLACE.items():
            seen_append = False
            for name, pos in calls:
                if name == append_name:
                    seen_append = True
                elif name == replace_name and seen_append:
                    line = src.count("\n", 0, pos) + 1
                    problems.append(
                        f"{rel}:{line}: `.{replace_name}(...)` follows "
                        f"`.{append_name}(...)` and discards it"
                    )
    return problems


def main() -> int:
    problems: list[str] = []
    files = 0
    for directory in SEARCH_DIRS:
        base = ROOT / directory
        if not base.is_dir():
            continue
        for path in sorted(base.rglob("*.rs")):
            files += 1
            problems.extend(scan(path))

    if not problems:
        print(
            f"append/replace setter ordering OK "
            f"({len(APPEND_THEN_REPLACE)} pairs, {files} files checked)."
        )
        return 0

    print("Replacing setter discards an earlier appending setter:\n", file=sys.stderr)
    for problem in problems:
        print(f"  {problem}", file=sys.stderr)
    print(
        "\n`children(...)` replaces the whole child list; `child(...)` appends one. "
        "Mixing them drops the appended entries silently. Use repeated `child(...)` "
        "calls, or move everything into the single `children(...)` call.",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
