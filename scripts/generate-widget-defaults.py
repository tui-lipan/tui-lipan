#!/usr/bin/env python3
"""Generate docs/widget-defaults.md from widget source initializers."""

from __future__ import annotations

import argparse
import difflib
import re
import sys
from dataclasses import dataclass
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "docs/widget-defaults.md"
FIX_COMMAND = "python3 scripts/generate-widget-defaults.py --write"


@dataclass(frozen=True)
class WidgetSpec:
    name: str
    source_type: str
    source_path: str
    extractor: str = "auto"
    notes: tuple[str, ...] = ()


WIDGETS: tuple[WidgetSpec, ...] = (
    WidgetSpec("Button", "Button", "src/widgets/button/mod.rs", "new"),
    WidgetSpec("Input", "Input", "src/widgets/input/mod.rs", "new"),
    WidgetSpec("TextArea", "TextArea", "src/widgets/text_area/mod.rs", "new"),
    WidgetSpec("List", "List", "src/widgets/list/mod.rs", "default"),
    WidgetSpec("ListConfig", "ListConfig", "src/widgets/list/mod.rs", "default"),
    WidgetSpec(
        "ScrollView",
        "ScrollView",
        "src/widgets/scroll_view/mod.rs",
        "default",
    ),
    WidgetSpec(
        "VStack/HStack shared StackProps",
        "StackProps",
        "src/widgets/containers/mod.rs",
        "default",
        ("Applies to the shared props backing stack container defaults.",),
    ),
    WidgetSpec(
        "Frame/FrameNode",
        "FrameNode",
        "src/widgets/frame/node.rs",
        "default",
        ("`Frame` is the public builder; these defaults come from its `FrameNode` backing type.",),
    ),
    WidgetSpec("ProgressBar", "ProgressBar", "src/widgets/progress/mod.rs", "new"),
    WidgetSpec("Spinner", "Spinner", "src/widgets/spinner/mod.rs", "new"),
    WidgetSpec("Select", "Select", "src/widgets/select/mod.rs", "new"),
    WidgetSpec("ComboBox", "ComboBox", "src/widgets/combo_box.rs", "new"),
    WidgetSpec("MultiSelect", "MultiSelect", "src/widgets/multi_select.rs", "new"),
    WidgetSpec("Modal", "Modal", "src/widgets/modal.rs", "new"),
    WidgetSpec("PanView", "PanView", "src/widgets/pan_view/mod.rs", "new"),
    WidgetSpec("Tabs", "Tabs", "src/widgets/tabs/mod.rs", "new"),
    WidgetSpec(
        "DraggableTabBar",
        "DraggableTabBar",
        "src/widgets/draggable_tab_bar/mod.rs",
        "new",
    ),
    WidgetSpec("DocumentView", "DocumentView", "src/widgets/document_view/mod.rs", "new"),
    WidgetSpec(
        "DiffView",
        "DiffView",
        "src/widgets/diff_view/mod.rs",
        "fn:new_internal",
        (
            "`DiffView::new()` delegates to `new_internal(\"\", \"\", None)`; "
            "argument-backed content fields are shown as constructor/local values.",
        ),
    ),
    WidgetSpec(
        "ManagedTerminal",
        "ManagedTerminalProps",
        "src/widgets/managed_terminal.rs",
        "default",
        ("`ManagedTerminal` is a component; defaults come from `ManagedTerminalProps`.",),
    ),
    WidgetSpec(
        "CommandPalette",
        "CommandPalette",
        "src/widgets/command_palette.rs",
        "new",
    ),
    WidgetSpec("ContextMenu", "ContextMenu", "src/widgets/context_menu.rs", "new"),
)


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


def line_number(text: str, offset: int) -> int:
    return text.count("\n", 0, offset) + 1


def find_matching(text: str, open_index: int) -> int:
    pairs = {"{": "}", "(": ")", "[": "]"}
    opener = text[open_index]
    closer = pairs[opener]
    depth = 0
    in_string = False
    in_char = False
    raw_hashes: int | None = None
    i = open_index
    while i < len(text):
        ch = text[i]
        prev = text[i - 1] if i else ""
        if raw_hashes is not None:
            if ch == '"' and text.startswith("#" * raw_hashes, i + 1):
                i += raw_hashes
                raw_hashes = None
        elif in_string:
            if ch == '"' and prev != "\\":
                in_string = False
        elif in_char:
            if ch == "'" and prev != "\\":
                in_char = False
        else:
            raw_match = re.match(r'r(#+)?"', text[i:])
            if raw_match:
                raw_hashes = len(raw_match.group(1) or "")
                i += raw_hashes + 1
            elif ch == '"':
                in_string = True
            elif ch == "'" and not re.match(r"'[A-Za-z_]", text[i:]):
                in_char = True
            elif ch == opener:
                depth += 1
            elif ch == closer:
                depth -= 1
                if depth == 0:
                    return i
        i += 1
    raise ValueError(f"unmatched {opener!r} at offset {open_index}")


def find_impl_block(text: str, type_name: str) -> tuple[int, int] | None:
    pattern = re.compile(rf"\bimpl(?:\s*<[^>]*>)?\s+{re.escape(type_name)}\s*{{")
    match = pattern.search(text)
    if not match:
        return None
    open_index = text.find("{", match.start())
    return open_index, find_matching(text, open_index)


def extract_default_initializer(text: str, type_name: str) -> tuple[str, int]:
    pattern = re.compile(rf"\bimpl\s+Default\s+for\s+{re.escape(type_name)}\s*{{")
    match = pattern.search(text)
    if not match:
        raise ValueError(f"impl Default for {type_name} not found")
    impl_open = text.find("{", match.start())
    impl_close = find_matching(text, impl_open)
    return extract_self_initializer(text, impl_open, impl_close)


def extract_new_initializer(text: str, type_name: str) -> tuple[str, int]:
    impl_block = find_impl_block(text, type_name)
    if impl_block is None:
        raise ValueError(f"impl {type_name} not found")
    impl_open, impl_close = impl_block
    body = text[impl_open:impl_close]
    match = re.search(r"\bpub\s+fn\s+new\s*(?:<[^>]*>\s*)?\(", body)
    if not match:
        raise ValueError(f"pub fn new for {type_name} not found")
    fn_start = impl_open + match.start()
    fn_open = text.find("{", fn_start, impl_close)
    if fn_open < 0:
        raise ValueError(f"pub fn new body for {type_name} not found")
    fn_close = find_matching(text, fn_open)
    return extract_self_initializer(text, fn_open, fn_close)


def extract_function_initializer(text: str, type_name: str, function_name: str) -> tuple[str, int]:
    impl_block = find_impl_block(text, type_name)
    if impl_block is None:
        raise ValueError(f"impl {type_name} not found")
    impl_open, impl_close = impl_block
    body = text[impl_open:impl_close]
    match = re.search(rf"\bfn\s+{re.escape(function_name)}\s*(?:<[^>]*>\s*)?\(", body)
    if not match:
        raise ValueError(f"fn {function_name} for {type_name} not found")
    fn_start = impl_open + match.start()
    fn_open = text.find("{", fn_start, impl_close)
    if fn_open < 0:
        raise ValueError(f"fn {function_name} body for {type_name} not found")
    fn_close = find_matching(text, fn_open)
    return extract_self_initializer(text, fn_open, fn_close)


def extract_self_initializer(text: str, start: int, end: int) -> tuple[str, int]:
    for match in re.finditer(r"\bSelf\s*{", text[start:end]):
        open_index = start + match.end() - 1
        close_index = find_matching(text, open_index)
        contents = text[open_index + 1 : close_index]
        # In `fn default() -> Self { Self { ... } }`, the return type's `Self {`
        # can look like an initializer. Skip the function body wrapper.
        if contents.lstrip().startswith("Self {"):
            continue
        return contents, line_number(text, open_index)
    raise ValueError("Self initializer not found")


def split_top_level(initializer: str) -> list[str]:
    items: list[str] = []
    start = 0
    depth = 0
    in_string = False
    in_char = False
    raw_hashes: int | None = None
    i = 0
    while i < len(initializer):
        ch = initializer[i]
        prev = initializer[i - 1] if i else ""
        if raw_hashes is not None:
            if ch == '"' and initializer.startswith("#" * raw_hashes, i + 1):
                i += raw_hashes
                raw_hashes = None
        elif in_string:
            if ch == '"' and prev != "\\":
                in_string = False
        elif in_char:
            if ch == "'" and prev != "\\":
                in_char = False
        else:
            raw_match = re.match(r'r(#+)?"', initializer[i:])
            if raw_match:
                raw_hashes = len(raw_match.group(1) or "")
                i += raw_hashes + 1
            elif ch == '"':
                in_string = True
            elif ch == "'" and not re.match(r"'[A-Za-z_]", initializer[i:]):
                in_char = True
            elif ch in "{([":
                depth += 1
            elif ch in "})]":
                depth -= 1
            elif ch == "," and depth == 0:
                item = initializer[start:i].strip()
                if item:
                    items.append(item)
                start = i + 1
        i += 1
    item = initializer[start:].strip()
    if item:
        items.append(item)
    return items


def normalize_value(value: str) -> str:
    value = re.sub(r"//.*", "", value)
    value = re.sub(r"\s+", " ", value.strip())
    value = value.replace(" { ", " { ").replace(" }", " }")
    return value


def parse_fields(initializer: str) -> list[tuple[str, str]]:
    fields: list[tuple[str, str]] = []
    for item in split_top_level(initializer):
        item = re.sub(r"(?m)^\s*//.*(?:\n|$)", "", item).strip()
        item = re.sub(r"(?m)^\s*#\[[^\n]+\]\s*(?:\n|$)", "", item).strip()
        if not item:
            continue
        if item.startswith(".."):
            fields.append(("..", normalize_value(item)))
            continue
        if ":" in item:
            field, value = item.split(":", 1)
            fields.append((field.strip(), normalize_value(value)))
        else:
            shorthand = item.strip()
            fields.append((shorthand, f"{shorthand} (shorthand local/argument)"))
    return fields


def merge_with_default_fields(
    text: str, type_name: str, fields: list[tuple[str, str]]
) -> list[tuple[str, str]]:
    if not any(field == ".." and value == "..Self::default()" for field, value in fields):
        return fields
    try:
        default_initializer, _line = extract_default_initializer(text, type_name)
    except ValueError:
        return fields

    default_fields = parse_fields(default_initializer)
    overrides = {field: value for field, value in fields if field != ".."}
    merged: list[tuple[str, str]] = []
    emitted: set[str] = set()
    for field, value in default_fields:
        if field in overrides:
            merged.append((field, overrides[field]))
            emitted.add(field)
        else:
            merged.append((field, value))
    for field, value in fields:
        if field != ".." and field not in emitted:
            merged.append((field, value))
    return merged


def source_link(path: str, line: int) -> str:
    return f"`{path}:{line}`"


def render_initializer(field: str, value: str) -> str:
    if value == f"{field} (shorthand local/argument)" or value in {
        f"{field}.into()",
        f"{field}.clamp(0.0, 1.0)",
        f"{field}.map(Arc::new)",
    }:
        return "constructor/local value (not a default)"
    return value


def extract_widget(spec: WidgetSpec) -> tuple[list[tuple[str, str]], int]:
    text = read(ROOT / spec.source_path)
    used_new = False
    try:
        if spec.extractor == "default":
            initializer, line = extract_default_initializer(text, spec.source_type)
        elif spec.extractor == "new":
            try:
                initializer, line = extract_new_initializer(text, spec.source_type)
                used_new = True
            except ValueError as exc:
                if "Self initializer not found" not in str(exc):
                    raise
                initializer, line = extract_default_initializer(text, spec.source_type)
        elif spec.extractor.startswith("fn:"):
            initializer, line = extract_function_initializer(
                text, spec.source_type, spec.extractor.removeprefix("fn:")
            )
        else:
            try:
                initializer, line = extract_default_initializer(text, spec.source_type)
            except ValueError:
                initializer, line = extract_new_initializer(text, spec.source_type)
    except ValueError as exc:
        raise SystemExit(f"error: {spec.source_path}: {exc}") from exc
    fields = parse_fields(initializer)
    if used_new:
        fields = merge_with_default_fields(text, spec.source_type, fields)
    return fields, line


def render() -> str:
    lines: list[str] = [
        "<!-- GENERATED BY scripts/generate-widget-defaults.py; DO NOT EDIT BY HAND. -->",
        "",
        "# Widget Defaults Reference",
        "",
        "This file is generated from widget source initializers (`Default` impls and `new` constructors).",
        "It is intended as an agent-facing quick reference for avoiding noisy builder chains.",
        "",
        "> **Agent rule:** omit setters whose value equals the default shown here. Do not emit `.width(Length::Auto)`, `.border(true)`, `.focusable(true)`, or similar calls unless the surrounding code needs to make that default explicit for readability.",
        "",
        f"Regenerate with `{FIX_COMMAND}`. Check freshness with `python3 scripts/generate-widget-defaults.py --check`.",
        "",
    ]
    for spec in WIDGETS:
        fields, line = extract_widget(spec)
        lines.extend(
            [
                f"## {spec.name}",
                "",
                f"Source: {source_link(spec.source_path, line)} (`{spec.source_type}`).",
                "",
            ]
        )
        for note in spec.notes:
            lines.extend([f"Note: {note}", ""])
        lines.extend(["| Field | Default initializer |", "|---|---|"])
        for field, value in fields:
            lines.append(f"| `{field}` | `{render_initializer(field, value)}` |")
        lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def print_diff(expected: str, actual: str) -> None:
    diff = difflib.unified_diff(
        actual.splitlines(keepends=True),
        expected.splitlines(keepends=True),
        fromfile=str(OUTPUT.relative_to(ROOT)),
        tofile=f"{OUTPUT.relative_to(ROOT)} (expected)",
    )
    sys.stderr.writelines(diff)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--write", action="store_true", help="write docs/widget-defaults.md")
    mode.add_argument("--check", action="store_true", help="verify docs/widget-defaults.md is current")
    args = parser.parse_args()

    expected = render()
    if args.write:
        write(OUTPUT, expected)
        return 0

    actual = read(OUTPUT) if OUTPUT.exists() else ""
    if actual != expected:
        print_diff(expected, actual)
        print(f"error: {OUTPUT.relative_to(ROOT)} is not current; run {FIX_COMMAND}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
