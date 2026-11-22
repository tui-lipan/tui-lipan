#!/usr/bin/env python3
"""Check crate-root export parity against src/widgets/mod.rs and src/prelude.rs.

Enforces three invariants textually across all feature gates:
1. Every public re-export in src/widgets/mod.rs is re-exported at the crate root
   (src/lib.rs) with matching #[cfg(...)] gates.
2. Every item exported by src/prelude.rs is also exported at the crate root
   (src/lib.rs), keeping `prelude ⊆ root`.
3. Generic collision-prone names (Msg, Step, State, Props, Event, Node, Item)
   are not exported into src/widgets/mod.rs or src/prelude.rs, and prelude
   widget re-exports do not rely on `as` rename workarounds.
"""

from __future__ import annotations

import re
import sys
from dataclasses import dataclass
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
LIB_RS = ROOT / "src/lib.rs"
PRELUDE_RS = ROOT / "src/prelude.rs"
WIDGETS_MOD_RS = ROOT / "src/widgets/mod.rs"

FORBIDDEN_GENERIC_EXPORTS = frozenset(
    {"Msg", "Step", "State", "Props", "Event", "Node", "Item"}
)


@dataclass(frozen=True)
class ExportEntry:
    name: str
    cfgs: frozenset[str]
    source: str
    is_aliased: bool


def read(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except OSError as exc:
        raise SystemExit(f"error: failed to read {path.relative_to(ROOT)}: {exc}") from exc


def strip_comments(text: str) -> str:
    # Remove line comments and block comments while keeping newlines for readability.
    out: list[str] = []
    i = 0
    n = len(text)
    in_string = False
    while i < n:
        ch = text[i]
        if in_string:
            out.append(ch)
            if ch == "\\" and i + 1 < n:
                out.append(text[i + 1])
                i += 2
                continue
            if ch == '"':
                in_string = False
            i += 1
            continue
        if ch == '"':
            in_string = True
            out.append(ch)
            i += 1
            continue
        if ch == "/" and i + 1 < n and text[i + 1] == "/":
            while i < n and text[i] != "\n":
                i += 1
            continue
        if ch == "/" and i + 1 < n and text[i + 1] == "*":
            depth = 1
            i += 2
            while i < n and depth > 0:
                if text[i] == "/" and i + 1 < n and text[i + 1] == "*":
                    depth += 1
                    i += 2
                elif text[i] == "*" and i + 1 < n and text[i + 1] == "/":
                    depth -= 1
                    i += 2
                else:
                    if text[i] == "\n":
                        out.append("\n")
                    i += 1
            continue
        out.append(ch)
        i += 1
    return "".join(out)


def normalize_cfg(attr: str) -> str:
    return re.sub(r"\s+", " ", attr.strip())


def split_top_level_commas(text: str) -> list[str]:
    parts: list[str] = []
    depth = 0
    start = 0
    for idx, ch in enumerate(text):
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
        elif ch == "," and depth == 0:
            part = text[start:idx].strip()
            if part:
                parts.append(part)
            start = idx + 1
    tail = text[start:].strip()
    if tail:
        parts.append(tail)
    return parts


def expand_use_tree(prefix: str, tree: str) -> list[tuple[str, str, bool]]:
    tree = tree.strip()
    if not tree:
        return []
    brace = tree.find("{")
    if brace != -1:
        head = tree[:brace].strip()
        close = tree.rfind("}")
        if close == -1:
            raise ValueError(f"unclosed brace in use tree: {tree!r}")
        combined = f"{prefix}{head}"
        results: list[tuple[str, str, bool]] = []
        for child in split_top_level_commas(tree[brace + 1 : close]):
            results.extend(expand_use_tree(combined, child))
        return results

    if " as " in tree:
        left, right = tree.rsplit(" as ", 1)
        return [(right.strip(), f"{prefix}{left.strip()}", True)]
    if tree == "self":
        leaf = prefix.rstrip(":").rsplit("::", 1)[-1]
        return [(leaf, prefix.rstrip(":"), False)]
    leaf = tree.rsplit("::", 1)[-1].strip()
    return [(leaf, f"{prefix}{tree}", False)]


def parse_top_level_exports(path: Path) -> dict[str, ExportEntry]:
    text = strip_comments(read(path))
    exports: dict[str, ExportEntry] = {}
    i = 0
    n = len(text)
    pending_attrs: list[str] = []

    def skip_ws(pos: int) -> int:
        while pos < n and text[pos].isspace():
            pos += 1
        return pos

    def read_balanced(pos: int, open_ch: str, close_ch: str) -> int:
        depth = 0
        in_str = False
        while pos < n:
            ch = text[pos]
            if in_str:
                if ch == "\\" and pos + 1 < n:
                    pos += 2
                    continue
                if ch == '"':
                    in_str = False
                pos += 1
                continue
            if ch == '"':
                in_str = True
            elif ch == open_ch:
                depth += 1
            elif ch == close_ch:
                depth -= 1
                if depth == 0:
                    return pos + 1
            pos += 1
        raise ValueError(f"unclosed {open_ch} in {path}")

    def skip_item_tail(pos: int) -> int:
        # Advance past either `;` or a balanced `{ ... }` block (or `( ... );`).
        in_str = False
        while pos < n:
            ch = text[pos]
            if in_str:
                if ch == "\\" and pos + 1 < n:
                    pos += 2
                    continue
                if ch == '"':
                    in_str = False
                pos += 1
                continue
            if ch == '"':
                in_str = True
                pos += 1
                continue
            if ch == ";":
                return pos + 1
            if ch == "{":
                return read_balanced(pos, "{", "}")
            pos += 1
        return pos

    while True:
        i = skip_ws(i)
        if i >= n:
            break

        # Inner crate/module attribute `#![...]`
        if text.startswith("#![", i):
            i = read_balanced(i + 2, "[", "]")
            pending_attrs.clear()
            continue

        # Outer attribute `#[...]`
        if text.startswith("#[", i):
            end = read_balanced(i + 1, "[", "]")
            pending_attrs.append(text[i:end])
            i = end
            continue

        cfgs = frozenset(
            normalize_cfg(a) for a in pending_attrs if a.startswith("#[cfg(")
        )
        is_macro_export = any("macro_export" in a for a in pending_attrs)

        # `pub use ...;` (explicitly exclude `pub(crate) use` etc.)
        if re.match(r"pub\s+use\b", text[i:]):
            semi = text.find(";", i)
            if semi == -1:
                raise ValueError(f"unterminated pub use in {path}")
            stmt = text[i:semi]
            body = re.sub(r"^pub\s+use\s+", "", stmt, count=1).strip()
            for name, src, is_aliased in expand_use_tree("", body):
                exports[name] = ExportEntry(
                    name=name, cfgs=cfgs, source=src, is_aliased=is_aliased
                )
            pending_attrs.clear()
            i = semi + 1
            continue

        # Top-level `pub (fn|struct|enum|type|const|static|mod|trait) <Name>`
        m_pub = re.match(
            r"pub\s+(?:async\s+)?(?:unsafe\s+)?(?:const\s+)?(?:fn|struct|enum|type|const|static|mod|trait)\s+([A-Za-z_][A-Za-z0-9_]*)",
            text[i:],
        )
        if m_pub:
            name = m_pub.group(1)
            if name not in exports:
                exports[name] = ExportEntry(
                    name=name, cfgs=cfgs, source=f"def:{name}", is_aliased=False
                )
            pending_attrs.clear()
            i = skip_item_tail(i + m_pub.end())
            continue

        # `#[macro_export] macro_rules! <Name>`
        m_macro = re.match(r"macro_rules!\s+([A-Za-z_][A-Za-z0-9_]*)", text[i:])
        if m_macro:
            if is_macro_export:
                name = m_macro.group(1)
                exports[name] = ExportEntry(
                    name=name, cfgs=cfgs, source=f"macro:{name}", is_aliased=False
                )
            pending_attrs.clear()
            i = skip_item_tail(i + m_macro.end())
            continue

        # Any other top-level item (private mod/fn/impl/use, pub(crate), etc.)
        pending_attrs.clear()
        i = skip_item_tail(i)

    return exports


def format_cfgs(cfgs: frozenset[str]) -> str:
    if not cfgs:
        return "(unconditional)"
    return " ".join(sorted(cfgs))


def main() -> int:
    lib_exports = parse_top_level_exports(LIB_RS)
    prelude_exports = parse_top_level_exports(PRELUDE_RS)
    widget_exports = parse_top_level_exports(WIDGETS_MOD_RS)

    diagnostics: list[str] = []

    # 1. Every public export in src/widgets/mod.rs must be re-exported from src/lib.rs.
    missing_widgets = sorted(widget_exports.keys() - lib_exports.keys())
    if missing_widgets:
        diagnostics.append(
            "src/lib.rs is missing public exports from src/widgets/mod.rs:\n  "
            + ", ".join(missing_widgets)
        )

    widget_cfg_mismatches = []
    for name in sorted(widget_exports.keys() & lib_exports.keys()):
        w_cfg = widget_exports[name].cfgs
        l_cfg = lib_exports[name].cfgs
        if w_cfg != l_cfg:
            widget_cfg_mismatches.append(
                f"  {name}: widgets/mod.rs has {format_cfgs(w_cfg)}, lib.rs has {format_cfgs(l_cfg)}"
            )
    if widget_cfg_mismatches:
        diagnostics.append(
            "src/lib.rs has #[cfg] mismatches against src/widgets/mod.rs:\n"
            + "\n".join(widget_cfg_mismatches)
        )

    # 2. Every item in src/prelude.rs must be exported from src/lib.rs (prelude ⊆ root).
    missing_prelude = sorted(prelude_exports.keys() - lib_exports.keys())
    if missing_prelude:
        diagnostics.append(
            "src/prelude.rs exports items not reachable at the crate root (src/lib.rs):\n  "
            + ", ".join(missing_prelude)
        )

    prelude_cfg_mismatches = []
    for name in sorted(prelude_exports.keys() & lib_exports.keys()):
        p_cfg = prelude_exports[name].cfgs
        l_cfg = lib_exports[name].cfgs
        if not l_cfg.issubset(p_cfg):
            prelude_cfg_mismatches.append(
                f"  {name}: prelude.rs has {format_cfgs(p_cfg)}, but lib.rs requires {format_cfgs(l_cfg)}"
            )
    if prelude_cfg_mismatches:
        diagnostics.append(
            "src/prelude.rs has narrower/incompatible #[cfg] gates than src/lib.rs:\n"
            + "\n".join(prelude_cfg_mismatches)
        )

    # 3. Forbid generic collision-prone names and widget rename-on-import aliases in prelude.
    bad_prelude = sorted(prelude_exports.keys() & FORBIDDEN_GENERIC_EXPORTS)
    if bad_prelude:
        diagnostics.append(
            "src/prelude.rs exports forbidden generic name(s): " + ", ".join(bad_prelude)
        )
    bad_widgets = sorted(widget_exports.keys() & FORBIDDEN_GENERIC_EXPORTS)
    if bad_widgets:
        diagnostics.append(
            "src/widgets/mod.rs exports forbidden generic name(s): " + ", ".join(bad_widgets)
        )
    aliased_prelude_widgets = sorted(
        entry.name
        for entry in prelude_exports.values()
        if entry.is_aliased and entry.source.startswith("crate::widgets::")
    )
    if aliased_prelude_widgets:
        diagnostics.append(
            "src/prelude.rs aliases widget export(s) with `as` (rename the type at its definition instead): "
            + ", ".join(aliased_prelude_widgets)
        )

    if diagnostics:
        print("Export parity check failed:\n", file=sys.stderr)
        print("\n\n".join(diagnostics), file=sys.stderr)
        return 1

    print(
        "Export parity OK "
        f"({len(widget_exports)} widget exports in root, "
        f"{len(prelude_exports)} prelude exports ⊆ {len(lib_exports)} root exports)."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
