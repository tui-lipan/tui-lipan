#!/usr/bin/env python3
"""Guard the "disabled widgets are never focusable" rule.

`Node::is_focusable`/`Node::is_tab_stop` exclude disabled widgets centrally, but only
for node kinds that report their state through `WidgetNode::is_disabled`. A node struct
that carries a `disabled` field without implementing that method silently keeps a dead
tab stop: Tab parks focus on a widget whose key handler refuses every key.

This check requires every widget node struct with a `disabled` field to implement
`is_disabled` in its `WidgetNode` impl, and flags widgets that re-derive the rule in
`is_focusable`/`is_tab_stop` instead of relying on the central one.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
WIDGETS = ROOT / "src" / "widgets"

DISABLED_FIELD = re.compile(r"^\s*pub(?:\(crate\))?\s+disabled\s*:\s*bool\s*,", re.M)
IMPL_HEADER = re.compile(r"impl\s+WidgetNode\s+for\s+(\w+)\s*\{")
IS_DISABLED = re.compile(r"fn\s+is_disabled\s*\(\s*&self\s*\)\s*->\s*bool\s*\{([^}]*)\}")
FOCUS_FN = re.compile(
    r"fn\s+(is_focusable|is_tab_stop)\s*\(\s*&self\s*\)\s*->\s*bool\s*\{([^}]*)\}"
)

# Node structs that hold a `disabled` field which must NOT gate focus. Keep this list
# narrow and give every entry a reason.
EXEMPT: dict[str, str] = {}

FIX_HINT = (
    "Add to the widget's `impl WidgetNode`:\n\n"
    "    fn is_disabled(&self) -> bool {\n"
    "        self.disabled\n"
    "    }\n\n"
    "`Node::is_focusable`/`Node::is_tab_stop` apply the rule from there, so "
    "`is_focusable`/`is_tab_stop` must not test `disabled` themselves."
)


def impl_body(src: str, start: int) -> str:
    """Return the body of the `impl` block whose opening brace precedes `start`."""
    depth = 0
    for index in range(start, len(src)):
        char = src[index]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return src[start:index]
    return src[start:]


def scan(path: Path) -> list[str]:
    src = path.read_text(encoding="utf-8")
    if not DISABLED_FIELD.search(src):
        return []

    rel = path.relative_to(ROOT).as_posix()
    problems: list[str] = []

    for match in IMPL_HEADER.finditer(src):
        node = match.group(1)
        if node in EXEMPT:
            continue
        body = impl_body(src, match.end() - 1)
        line = src.count("\n", 0, match.start()) + 1

        reported = IS_DISABLED.search(body)
        if not reported:
            problems.append(
                f"{rel}:{line}: `{node}` has a `disabled` field but no `is_disabled`, "
                f"so it stays focusable while disabled"
            )
        elif "self.disabled" not in reported.group(1):
            problems.append(
                f"{rel}:{line}: `{node}::is_disabled` does not report `self.disabled`"
            )

        for focus_match in FOCUS_FN.finditer(body):
            if "disabled" in focus_match.group(2):
                problems.append(
                    f"{rel}:{line}: `{node}::{focus_match.group(1)}` tests `disabled` "
                    f"itself; the central rule in `Node::is_focusable` already does"
                )

    return problems


def main() -> int:
    problems: list[str] = []
    nodes = 0
    for path in sorted(WIDGETS.rglob("*.rs")):
        if not DISABLED_FIELD.search(path.read_text(encoding="utf-8")):
            continue
        nodes += 1
        problems.extend(scan(path))

    if not problems:
        print(f"disabled/focus rule OK ({nodes} files with a `disabled` field checked).")
        return 0

    print("Disabled widgets must be excluded from focus:\n", file=sys.stderr)
    for problem in problems:
        print(f"  {problem}", file=sys.stderr)
    print(f"\n{FIX_HINT}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
