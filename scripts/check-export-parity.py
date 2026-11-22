#!/usr/bin/env python3
"""Check crate-root export parity against src/widgets/mod.rs and src/prelude.rs.

Enforces three invariants textually across all feature gates:
1. Every public export in src/widgets/mod.rs is re-exported at the crate root
   (src/lib.rs) with matching #[cfg(...)] gates and resolves to the same
   canonical item.
2. Every item exported by src/prelude.rs is exported at the crate root
   (src/lib.rs) with compatible #[cfg(...)] gates and resolves to the same
   canonical item (`prelude ⊆ root`).
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

PUB_ITEM_RE = re.compile(
    r"pub\s+(?:async\s+)?(?:unsafe\s+)?(?:const\s+)?"
    r"(?:fn|struct|enum|type|const|static|mod|trait)\s+([A-Za-z_][A-Za-z0-9_]*)"
)
MOD_DECL_RE = re.compile(
    r"(?:pub(?:\s*\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\b"
)
MACRO_RE = re.compile(r"macro_rules!\s+([A-Za-z_][A-Za-z0-9_]*)")


@dataclass(frozen=True)
class ExportEntry:
    name: str
    cfgs: frozenset[str]
    source: str
    is_def: bool
    is_aliased: bool


class ResolutionError(ValueError):
    pass


def strip_comments(text: str) -> str:
    out: list[str] = []
    i, n, in_str = 0, len(text), False
    while i < n:
        ch = text[i]
        if in_str:
            out.append(ch)
            if ch == "\\" and i + 1 < n:
                out.append(text[i + 1])
                i += 2
                continue
            in_str = ch != '"'
            i += 1
        elif ch == '"':
            in_str = True
            out.append(ch)
            i += 1
        elif text.startswith("//", i):
            nl = text.find("\n", i)
            i = n if nl == -1 else nl
        elif text.startswith("/*", i):
            depth, i = 1, i + 2
            while i < n and depth > 0:
                if text.startswith("/*", i):
                    depth, i = depth + 1, i + 2
                elif text.startswith("*/", i):
                    depth, i = depth - 1, i + 2
                else:
                    if text[i] == "\n":
                        out.append("\n")
                    i += 1
        else:
            out.append(ch)
            i += 1
    return "".join(out)


def split_top_level(text: str) -> list[str]:
    parts: list[str] = []
    depth, start = 0, 0
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
    if tail := text[start:].strip():
        parts.append(tail)
    return parts


def expand_use_tree(prefix: str, tree: str) -> list[tuple[str, str, bool]]:
    tree = tree.strip()
    if not tree:
        return []
    if (brace := tree.find("{")) != -1:
        close = tree.rfind("}")
        if close == -1:
            raise ResolutionError(f"unclosed brace in use tree: {tree!r}")
        combined = f"{prefix}{tree[:brace].strip()}"
        return [
            item
            for child in split_top_level(tree[brace + 1 : close])
            for item in expand_use_tree(combined, child)
        ]
    if " as " in tree:
        left, right = tree.rsplit(" as ", 1)
        return [(right.strip(), f"{prefix}{left.strip()}", True)]
    if tree == "self":
        base = prefix.rstrip(":")
        return [(base.rsplit("::", 1)[-1], base, False)]
    return [(tree.rsplit("::", 1)[-1].strip(), f"{prefix}{tree}", False)]


def parse_module(text: str, mod_path: str) -> tuple[dict[str, ExportEntry], set[str]]:
    src = strip_comments(text)
    exports: dict[str, ExportEntry] = {}
    local_mods: set[str] = set()
    i, n = 0, len(src)
    attrs: list[str] = []

    def add(entry: ExportEntry) -> None:
        if entry.name in exports:
            raise ResolutionError(f"`{mod_path}` exports `{entry.name}` more than once")
        exports[entry.name] = entry

    def read_balanced(pos: int, open_ch: str, close_ch: str) -> int:
        depth, in_str = 0, False
        while pos < n:
            ch = src[pos]
            if in_str:
                if ch == "\\" and pos + 1 < n:
                    pos += 2
                    continue
                in_str = ch != '"'
            elif ch == '"':
                in_str = True
            elif ch == open_ch:
                depth += 1
            elif ch == close_ch:
                depth -= 1
                if depth == 0:
                    return pos + 1
            pos += 1
        raise ResolutionError(f"unclosed {open_ch} in {mod_path}")

    def skip_tail(pos: int) -> int:
        in_str = False
        while pos < n:
            ch = src[pos]
            if in_str:
                if ch == "\\" and pos + 1 < n:
                    pos += 2
                    continue
                in_str = ch != '"'
            elif ch == '"':
                in_str = True
            elif ch == ";":
                return pos + 1
            elif ch == "{":
                return read_balanced(pos, "{", "}")
            pos += 1
        return pos

    while i < n:
        while i < n and src[i].isspace():
            i += 1
        if i >= n:
            break
        if src.startswith("#![", i):
            i = read_balanced(i + 2, "[", "]")
            attrs.clear()
            continue
        if src.startswith("#[", i):
            end = read_balanced(i + 1, "[", "]")
            attrs.append(src[i:end])
            i = end
            continue

        cfgs = frozenset(
            re.sub(r"\s+", " ", a.strip()) for a in attrs if a.startswith("#[cfg(")
        )
        is_macro_export = any("macro_export" in a for a in attrs)
        attrs.clear()

        if m_mod := MOD_DECL_RE.match(src, i):
            local_mods.add(m_mod.group(1))

        if m_use := re.match(r"pub\s+use\s+", src[i:]):
            semi = src.find(";", i)
            if semi == -1:
                raise ResolutionError(f"unterminated pub use in {mod_path}")
            body = src[i + m_use.end() : semi].strip()
            for name, raw_src, is_aliased in expand_use_tree("", body):
                add(ExportEntry(name, cfgs, raw_src, False, is_aliased))
            i = semi + 1
        elif m_pub := PUB_ITEM_RE.match(src, i):
            name = m_pub.group(1)
            add(ExportEntry(name, cfgs, f"{mod_path}::{name}", True, False))
            i = skip_tail(m_pub.end())
        elif m_macro := MACRO_RE.match(src, i):
            if is_macro_export:
                name = m_macro.group(1)
                add(ExportEntry(name, cfgs, f"{mod_path}::{name}", True, False))
            i = skip_tail(m_macro.end())
        else:
            i = skip_tail(i)

    return exports, local_mods


def normalize_source(raw: str, mod_path: str, local_mods: set[str]) -> str:
    path = re.sub(r"\s+", "", raw)
    if not path or "*" in path or ":::" in path or path.endswith("::"):
        raise ResolutionError(f"unsupported or malformed path `{raw}`")
    if path.startswith("::"):
        return f"ext::{path[2:]}"
    if path.startswith("crate::"):
        return path
    if path.startswith("self::"):
        return f"{mod_path}::{path[6:]}"
    if path.startswith("super::"):
        parts = mod_path.split("::")
        rest = path
        while rest.startswith("super::"):
            if len(parts) <= 1:
                raise ResolutionError(f"`{raw}` escapes crate root from `{mod_path}`")
            parts.pop()
            rest = rest[7:]
        return f"{'::'.join(parts)}::{rest}"
    head = path.split("::", 1)[0]
    if head in local_mods:
        return f"{mod_path}::{path}"
    if "::" in path:
        return f"ext::{path}"
    raise ResolutionError(f"unresolvable bare path `{raw}` in `{mod_path}`")


def canonicalize(
    entry: ExportEntry,
    mod_path: str,
    tables: dict[str, tuple[dict[str, ExportEntry], set[str]]],
) -> str:
    if entry.is_def:
        return entry.source
    path = normalize_source(entry.source, mod_path, tables[mod_path][1])
    seen: set[str] = set()
    while True:
        if path in seen:
            raise ResolutionError(f"re-export cycle at `{path}`")
        seen.add(path)
        if "::" not in path:
            raise ResolutionError(f"cannot resolve `{path}`")
        parent, leaf = path.rsplit("::", 1)
        if parent not in tables:
            return path
        parent_exports, parent_mods = tables[parent]
        target = parent_exports.get(leaf)
        if target is None:
            raise ResolutionError(f"`{path}` is not exported by `{parent}`")
        if target.is_def:
            return target.source
        path = normalize_source(target.source, parent, parent_mods)


def format_cfgs(cfgs: frozenset[str]) -> str:
    return " ".join(sorted(cfgs)) if cfgs else "(unconditional)"


def check_sources(lib_src: str, prelude_src: str, widgets_src: str) -> tuple[list[str], tuple[int, int, int]]:
    lib_exports, lib_mods = parse_module(lib_src, "crate")
    prelude_exports, prelude_mods = parse_module(prelude_src, "crate::prelude")
    widget_exports, widget_mods = parse_module(widgets_src, "crate::widgets")
    tables = {
        "crate": (lib_exports, lib_mods),
        "crate::prelude": (prelude_exports, prelude_mods),
        "crate::widgets": (widget_exports, widget_mods),
    }

    diagnostics: list[str] = []
    resolved: dict[str, dict[str, str]] = {mod: {} for mod in tables}
    unresolved: list[str] = []
    for mod, (exports, _) in tables.items():
        for name, entry in sorted(exports.items()):
            try:
                resolved[mod][name] = canonicalize(entry, mod, tables)
            except ResolutionError as exc:
                unresolved.append(f"  {mod}::{name}: {exc}")
    if unresolved:
        diagnostics.append("Unresolvable export path(s):\n" + "\n".join(unresolved))

    # 1. Every public export in src/widgets/mod.rs must be re-exported at root with matching cfg and target.
    if missing := sorted(widget_exports.keys() - lib_exports.keys()):
        diagnostics.append(
            "src/lib.rs is missing public exports from src/widgets/mod.rs:\n  "
            + ", ".join(missing)
        )
    widget_cfg_errs: list[str] = []
    widget_target_errs: list[str] = []
    for name in sorted(widget_exports.keys() & lib_exports.keys()):
        w_cfg, l_cfg = widget_exports[name].cfgs, lib_exports[name].cfgs
        if w_cfg != l_cfg:
            widget_cfg_errs.append(
                f"  {name}: widgets/mod.rs has {format_cfgs(w_cfg)}, lib.rs has {format_cfgs(l_cfg)}"
            )
        w_canon = resolved["crate::widgets"].get(name)
        l_canon = resolved["crate"].get(name)
        if w_canon and l_canon and w_canon != l_canon:
            widget_target_errs.append(
                f"  {name}: widgets/mod.rs resolves to `{w_canon}`, lib.rs resolves to `{l_canon}`"
            )
    if widget_cfg_errs:
        diagnostics.append(
            "src/lib.rs has #[cfg] mismatches against src/widgets/mod.rs:\n"
            + "\n".join(widget_cfg_errs)
        )
    if widget_target_errs:
        diagnostics.append(
            "src/lib.rs exports a different item than src/widgets/mod.rs:\n"
            + "\n".join(widget_target_errs)
        )

    # 2. Every item in src/prelude.rs must be exported at root with compatible cfg and same target.
    if missing := sorted(prelude_exports.keys() - lib_exports.keys()):
        diagnostics.append(
            "src/prelude.rs exports items not reachable at the crate root (src/lib.rs):\n  "
            + ", ".join(missing)
        )
    prelude_cfg_errs: list[str] = []
    prelude_target_errs: list[str] = []
    for name in sorted(prelude_exports.keys() & lib_exports.keys()):
        p_cfg, l_cfg = prelude_exports[name].cfgs, lib_exports[name].cfgs
        if not l_cfg.issubset(p_cfg):
            prelude_cfg_errs.append(
                f"  {name}: prelude.rs has {format_cfgs(p_cfg)}, but lib.rs requires {format_cfgs(l_cfg)}"
            )
        p_canon = resolved["crate::prelude"].get(name)
        l_canon = resolved["crate"].get(name)
        if p_canon and l_canon and p_canon != l_canon:
            prelude_target_errs.append(
                f"  {name}: prelude.rs resolves to `{p_canon}`, lib.rs resolves to `{l_canon}`"
            )
    if prelude_cfg_errs:
        diagnostics.append(
            "src/prelude.rs has narrower/incompatible #[cfg] gates than src/lib.rs:\n"
            + "\n".join(prelude_cfg_errs)
        )
    if prelude_target_errs:
        diagnostics.append(
            "src/prelude.rs exports a different item than src/lib.rs:\n"
            + "\n".join(prelude_target_errs)
        )

    # 3. Forbid generic collision-prone names and widget rename-on-import aliases in prelude.
    if bad := sorted(prelude_exports.keys() & FORBIDDEN_GENERIC_EXPORTS):
        diagnostics.append("src/prelude.rs exports forbidden generic name(s): " + ", ".join(bad))
    if bad := sorted(widget_exports.keys() & FORBIDDEN_GENERIC_EXPORTS):
        diagnostics.append("src/widgets/mod.rs exports forbidden generic name(s): " + ", ".join(bad))
    aliased_widgets = sorted(
        e.name
        for e in prelude_exports.values()
        if e.is_aliased and re.sub(r"\s+", "", e.source).startswith(("crate::widgets::", "widgets::"))
    )
    if aliased_widgets:
        diagnostics.append(
            "src/prelude.rs aliases widget export(s) with `as` (rename the type at its definition instead): "
            + ", ".join(aliased_widgets)
        )

    return diagnostics, (len(widget_exports), len(prelude_exports), len(lib_exports))


def main() -> int:
    try:
        lib_src = LIB_RS.read_text(encoding="utf-8")
        prelude_src = PRELUDE_RS.read_text(encoding="utf-8")
        widgets_src = WIDGETS_MOD_RS.read_text(encoding="utf-8")
    except OSError as exc:
        print(f"error: failed to read source file: {exc}", file=sys.stderr)
        return 2

    try:
        diagnostics, (n_widgets, n_prelude, n_lib) = check_sources(
            lib_src, prelude_src, widgets_src
        )
    except ResolutionError as exc:
        print(f"Export parity check failed:\n\n{exc}", file=sys.stderr)
        return 1

    if diagnostics:
        print("Export parity check failed:\n", file=sys.stderr)
        print("\n\n".join(diagnostics), file=sys.stderr)
        return 1

    print(
        f"Export parity OK ({n_widgets} widget exports in root, "
        f"{n_prelude} prelude exports ⊆ {n_lib} root exports)."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
