#!/usr/bin/env python3
"""Guard widget state styles against raw Style fields.

State overlays such as hover/focus/active/selection should use StyleSlot so
widgets can distinguish replacing a theme role from extending or inheriting it.
This check flags newly-added `*_style` fields typed as bare Style unless the
field is an explicitly allowlisted non-state style or legacy raw state style.
"""

from __future__ import annotations

import re
import sys
from dataclasses import dataclass
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
WIDGETS = ROOT / "src" / "widgets"


# Exact `(relative path, field name)` entries that are intentionally raw Style.
# Keep this list narrow and include a reason for every entry.
ALLOWLIST: dict[tuple[str, str], str] = {
    ("src/widgets/accordion/mod.rs", "content_style"): "non-state visual part style",
    ("src/widgets/accordion/mod.rs", "disabled_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/accordion/mod.rs", "header_style"): "non-state visual part style",
    ("src/widgets/badge.rs", "text_style"): "non-state visual part style",
    ("src/widgets/breadcrumb.rs", "active_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/breadcrumb.rs", "inactive_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/breadcrumb.rs", "separator_style"): "non-state visual part style",
    ("src/widgets/button/mod.rs", "disabled_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/button/mod.rs", "icon_style"): "non-state visual part style",
    ("src/widgets/button/mod.rs", "shortcut_style"): "non-state visual part style",
    ("src/widgets/button/node.rs", "disabled_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/button/node.rs", "icon_style"): "non-state visual part style",
    ("src/widgets/button/node.rs", "shortcut_style"): "non-state visual part style",
    ("src/widgets/chart/mod.rs", "axis_style"): "non-state visual part style",
    ("src/widgets/chart/mod.rs", "grid_style"): "non-state visual part style",
    ("src/widgets/chart/mod.rs", "legend_style"): "non-state visual part style",
    ("src/widgets/chart/node.rs", "axis_style"): "non-state visual part style",
    ("src/widgets/chart/node.rs", "grid_style"): "non-state visual part style",
    ("src/widgets/chart/node.rs", "legend_style"): "non-state visual part style",
    ("src/widgets/checkbox/mod.rs", "checked_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/checkbox/mod.rs", "disabled_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/checkbox/mod.rs", "indeterminate_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/checkbox/mod.rs", "label_style"): "non-state visual part style",
    ("src/widgets/checkbox/mod.rs", "unchecked_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/checkbox/node.rs", "checked_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/checkbox/node.rs", "disabled_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/checkbox/node.rs", "indeterminate_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/checkbox/node.rs", "label_style"): "non-state visual part style",
    ("src/widgets/checkbox/node.rs", "unchecked_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/class_diagram/mod.rs", "class_style"): "class diagram node visual part style",
    ("src/widgets/class_diagram/mod.rs", "edge_style"): "class diagram edge visual part style",
    ("src/widgets/class_diagram/mod.rs", "label_style"): "class diagram label visual part style",
    ("src/widgets/combo_box.rs", "input_disabled_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/combo_box.rs", "input_focus_content_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/combo_box.rs", "input_focus_placeholder_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/combo_box.rs", "input_focus_suffix_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/combo_box.rs", "input_placeholder_style"): "non-state visual part style",
    ("src/widgets/combo_box.rs", "input_style"): "non-state visual part style",
    ("src/widgets/combo_box.rs", "input_suffix_style"): "non-state visual part style",
    ("src/widgets/command_palette.rs", "backdrop_style"): "non-state visual part style",
    ("src/widgets/command_palette.rs", "frame_style"): "non-state visual part style",
    ("src/widgets/command_palette.rs", "title_style"): "non-state visual part style",
    ("src/widgets/common/simple_diagram.rs", "fill_style"): "shared diagram fill visual part style",
    ("src/widgets/common/simple_diagram.rs", "label_style"): "shared diagram label visual part style",
    ("src/widgets/common/simple_diagram.rs", "line_style"): "shared diagram line visual part style",
    ("src/widgets/containers/mod.rs", "inactive_tab_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/containers/node.rs", "inactive_tab_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/context_menu.rs", "selection_symbol_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/context_menu.rs", "unfocused_selection_symbol_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/date_picker/mod.rs", "day_style"): "non-state visual part style",
    ("src/widgets/date_picker/mod.rs", "header_style"): "non-state visual part style",
    ("src/widgets/date_picker/mod.rs", "nav_disabled_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/date_picker/mod.rs", "nav_style"): "non-state visual part style",
    ("src/widgets/date_picker/mod.rs", "outside_month_style"): "non-state visual part style",
    ("src/widgets/date_picker/mod.rs", "selected_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/date_picker/mod.rs", "title_style"): "non-state visual part style",
    ("src/widgets/date_picker/mod.rs", "weekday_style"): "non-state visual part style",
    ("src/widgets/diff_view/mod.rs", "context_separator_hover_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/diff_view/mod.rs", "vertical_separator_style"): "non-state visual part style",
    ("src/widgets/diff_view/types.rs", "hover_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/document_view/format.rs", "blockquote_bar_style"): "non-state visual part style",
    ("src/widgets/document_view/format.rs", "code_block_style"): "non-state visual part style",
    ("src/widgets/document_view/format.rs", "code_inline_style"): "non-state visual part style",
    ("src/widgets/document_view/format.rs", "diagram_edge_style"): "document diagram edge visual part style",
    ("src/widgets/document_view/format.rs", "diagram_muted_style"): "document diagram muted visual part style",
    ("src/widgets/document_view/format.rs", "diagram_node_border_style"): "document diagram node border visual part style",
    ("src/widgets/document_view/format.rs", "diagram_node_fill_style"): "document diagram node fill visual part style",
    ("src/widgets/document_view/format.rs", "diagram_node_label_style"): "document diagram node label visual part style",
    ("src/widgets/document_view/format.rs", "emphasis_style"): "non-state visual part style",
    ("src/widgets/document_view/format.rs", "hr_style"): "non-state visual part style",
    ("src/widgets/document_view/format.rs", "link_style"): "non-state visual part style",
    ("src/widgets/document_view/format.rs", "list_enumeration_style"): "non-state visual part style",
    ("src/widgets/document_view/format.rs", "list_item_style"): "non-state visual part style",
    ("src/widgets/document_view/format.rs", "strikethrough_style"): "non-state visual part style",
    ("src/widgets/document_view/format.rs", "strong_style"): "non-state visual part style",
    ("src/widgets/document_view/format.rs", "table_border_style"): "non-state visual part style",
    ("src/widgets/document_view/format.rs", "table_header_style"): "non-state visual part style",
    ("src/widgets/document_view/mod.rs", "focus_content_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/document_view/mod.rs", "line_number_style"): "non-state visual part style",
    ("src/widgets/document_view/mod.rs", "split_wrap_padding_style"): "split-wrap padding visual part style",
    ("src/widgets/document_view/mod.rs", "split_wrap_padding_gutter_style"): "non-state visual part style",
    ("src/widgets/document_view/node.rs", "focus_content_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/document_view/node.rs", "line_number_style"): "non-state visual part style",
    ("src/widgets/document_view/node.rs", "scrollbar_thumb_focus_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/document_view/node.rs", "scrollbar_thumb_style"): "non-state visual part style",
    ("src/widgets/document_view/node.rs", "scrollbar_track_style"): "non-state visual part style",
    ("src/widgets/document_view/node.rs", "split_wrap_padding_style"): "split-wrap padding visual part style",
    ("src/widgets/document_view/node.rs", "split_wrap_padding_gutter_style"): "non-state visual part style",
    ("src/widgets/draggable_tab_bar/mod.rs", "accent_style"): "non-state visual part style",
    ("src/widgets/draggable_tab_bar/mod.rs", "active_accent_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/draggable_tab_bar/mod.rs", "active_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/draggable_tab_bar/mod.rs", "close_hover_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/draggable_tab_bar/mod.rs", "close_style"): "non-state visual part style",
    ("src/widgets/draggable_tab_bar/mod.rs", "disabled_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/draggable_tab_bar/mod.rs", "hover_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/draggable_tab_bar/mod.rs", "overflow_hover_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/draggable_tab_bar/mod.rs", "overflow_style"): "non-state visual part style",
    ("src/widgets/draggable_tab_bar/node.rs", "accent_style"): "non-state visual part style",
    ("src/widgets/draggable_tab_bar/node.rs", "active_accent_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/draggable_tab_bar/node.rs", "close_hover_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/draggable_tab_bar/node.rs", "close_style"): "non-state visual part style",
    ("src/widgets/draggable_tab_bar/node.rs", "disabled_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/draggable_tab_bar/node.rs", "overflow_hover_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/draggable_tab_bar/node.rs", "overflow_style"): "non-state visual part style",
    ("src/widgets/er_diagram/mod.rs", "edge_style"): "ER diagram edge visual part style",
    ("src/widgets/er_diagram/mod.rs", "entity_style"): "ER diagram entity visual part style",
    ("src/widgets/er_diagram/mod.rs", "label_style"): "ER diagram label visual part style",
    ("src/widgets/file_tree/mod_private.rs", "empty_text_style"): "non-state visual part style",
    ("src/widgets/file_tree/mod_private.rs", "directory_label_style"): "file-tree label visual part style",
    ("src/widgets/file_tree/mod_private.rs", "file_label_style"): "file-tree label visual part style",
    ("src/widgets/file_tree/mod_private.rs", "change_suffix_style"): "file-tree suffix visual part style",
    ("src/widgets/file_tree/mod_private.rs", "explorer_divider_style"): "non-state visual part style",
    ("src/widgets/file_tree/mod_private.rs", "explorer_focus_placeholder_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/file_tree/mod_private.rs", "explorer_input_focus_content_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/file_tree/mod_private.rs", "explorer_input_style"): "non-state visual part style",
    ("src/widgets/file_tree/mod_private.rs", "explorer_match_style"): "non-state visual part style",
    ("src/widgets/file_tree/mod_private.rs", "explorer_placeholder_style"): "non-state visual part style",
    ("src/widgets/file_tree/mod_private.rs", "indent_guide_style"): "non-state visual part style",
    ("src/widgets/file_tree/mod_private.rs", "scroll_indicator_style"): "non-state visual part style",
    ("src/widgets/file_tree/mod_private.rs", "selection_symbol_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/file_tree/mod_private.rs", "unfocused_selection_symbol_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/frame/mod.rs", "focus_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/frame/mod.rs", "hover_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/frame/node.rs", "active_tab_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/frame/node.rs", "focus_active_tab_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/frame/node.rs", "focus_inactive_tab_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/frame/node.rs", "focus_status_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/frame/node.rs", "focus_title_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/frame/node.rs", "inactive_tab_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/frame/node.rs", "inner_style"): "non-state visual part style",
    ("src/widgets/frame/node.rs", "status_style"): "non-state visual part style",
    ("src/widgets/frame/node.rs", "title_style"): "non-state visual part style",
    ("src/widgets/graph/mod.rs", "edge_style"): "non-state visual part style",
    ("src/widgets/graph/mod.rs", "hover_style"): "graph item hover overlay style; TODO migrate to StyleSlot",
    ("src/widgets/graph/mod.rs", "node_hover_style"): "non-state visual part style",
    ("src/widgets/graph/mod.rs", "node_style"): "non-state visual part style",
    ("src/widgets/graph/node.rs", "edge_style"): "non-state visual part style",
    ("src/widgets/graph/node.rs", "hover_style"): "graph item hover overlay style; TODO migrate to StyleSlot",
    ("src/widgets/graph/node.rs", "node_hover_style"): "non-state visual part style",
    ("src/widgets/graph/node.rs", "node_style"): "non-state visual part style",
    ("src/widgets/flowchart/mod.rs", "edge_style"): "non-state visual part style",
    ("src/widgets/flowchart/mod.rs", "hover_style"): "flowchart item hover overlay style; TODO migrate to StyleSlot",
    ("src/widgets/flowchart/mod.rs", "item_hover_style"): "flowchart item hover overlay style; TODO migrate to StyleSlot",
    ("src/widgets/flowchart/mod.rs", "label_style"): "non-state visual part style",
    ("src/widgets/flowchart/mod.rs", "line_style"): "per-edge visual part style",
    ("src/widgets/flowchart/mod.rs", "node_style"): "non-state visual part style",
    ("src/widgets/flowchart/mod.rs", "subgraph_style"): "non-state visual part style",
    ("src/widgets/flowchart/node.rs", "edge_style"): "non-state visual part style",
    ("src/widgets/flowchart/node.rs", "hover_style"): "flowchart item hover overlay style; TODO migrate to StyleSlot",
    ("src/widgets/flowchart/node.rs", "item_hover_style"): "flowchart item hover overlay style; TODO migrate to StyleSlot",
    ("src/widgets/flowchart/node.rs", "label_style"): "non-state visual part style",
    ("src/widgets/flowchart/node.rs", "line_style"): "per-edge visual part style",
    ("src/widgets/flowchart/node.rs", "node_style"): "non-state visual part style",
    ("src/widgets/flowchart/node.rs", "subgraph_style"): "non-state visual part style",
    ("src/widgets/flowchart/theme.rs", "header_style"): "flowchart subgraph header visual part style",
    ("src/widgets/flowchart/theme.rs", "item_hover_style"): "flowchart theme hover overlay style; TODO migrate to StyleSlot",
    ("src/widgets/flowchart/theme.rs", "label_style"): "flowchart edge label visual part style",
    ("src/widgets/sequence_diagram/mod.rs", "label_style"): "planned raw per-message visual part style",
    ("src/widgets/sequence_diagram/mod.rs", "line_style"): "planned raw per-message visual part style",
    ("src/widgets/sequence_diagram/node.rs", "label_style"): "planned raw per-message visual part style",
    ("src/widgets/sequence_diagram/node.rs", "line_style"): "planned raw per-message visual part style",
    ("src/widgets/sequence_diagram/theme.rs", "hover_style"): "sequence diagram theme hover overlay style; TODO migrate to StyleSlot if themes gain roles",
    ("src/widgets/sequence_diagram/theme.rs", "message_label_style"): "sequence diagram theme visual part style",
    ("src/widgets/sequence_diagram/theme.rs", "note_style"): "sequence diagram theme visual part style",
    ("src/widgets/sequence_diagram/theme.rs", "participant_style"): "sequence diagram theme visual part style",
    ("src/widgets/state_diagram/mod.rs", "edge_style"): "state diagram edge visual part style",
    ("src/widgets/state_diagram/mod.rs", "label_style"): "state diagram label visual part style",
    ("src/widgets/state_diagram/mod.rs", "state_style"): "state diagram node visual part style",
    ("src/widgets/heatmap/mod.rs", "label_style"): "non-state visual part style",
    ("src/widgets/heatmap/mod.rs", "legend_style"): "non-state visual part style",
    ("src/widgets/heatmap/node.rs", "label_style"): "non-state visual part style",
    ("src/widgets/heatmap/node.rs", "legend_style"): "non-state visual part style",
    ("src/widgets/hex_area/mod.rs", "cursor_style"): "non-state visual part style",
    ("src/widgets/hex_area/mod.rs", "disabled_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/hex_area/mod.rs", "focus_content_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/hex_area/mod.rs", "pending_edit_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/hex_area/node.rs", "cursor_style"): "non-state visual part style",
    ("src/widgets/hex_area/node.rs", "disabled_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/hex_area/node.rs", "focus_content_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/hex_area/node.rs", "pending_edit_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/hyperlink.rs", "disabled_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/hyperlink.rs", "visited_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/input/mod.rs", "disabled_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/input/mod.rs", "error_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/input/mod.rs", "focus_content_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/input/mod.rs", "focus_placeholder_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/input/mod.rs", "focus_prefix_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/input/mod.rs", "focus_suffix_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/input/mod.rs", "placeholder_style"): "non-state visual part style",
    ("src/widgets/input/mod.rs", "prefix_style"): "non-state visual part style",
    ("src/widgets/input/mod.rs", "suffix_style"): "non-state visual part style",
    ("src/widgets/input/node.rs", "disabled_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/input/node.rs", "error_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/input/node.rs", "focus_content_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/input/node.rs", "focus_placeholder_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/input/node.rs", "focus_prefix_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/input/node.rs", "focus_suffix_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/input/node.rs", "placeholder_style"): "non-state visual part style",
    ("src/widgets/input/node.rs", "prefix_style"): "non-state visual part style",
    ("src/widgets/input/node.rs", "suffix_style"): "non-state visual part style",
    ("src/widgets/list/mod.rs", "active_symbol_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/list/mod.rs", "disabled_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/list/mod.rs", "empty_text_style"): "non-state visual part style",
    ("src/widgets/list/mod.rs", "label_style"): "non-state visual part style",
    ("src/widgets/list/mod.rs", "prefix_style"): "non-state visual part style",
    ("src/widgets/list/mod.rs", "scroll_indicator_style"): "non-state visual part style",
    ("src/widgets/list/mod.rs", "selection_symbol_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/list/mod.rs", "title_style"): "non-state visual part style",
    ("src/widgets/list/mod.rs", "unfocused_selection_symbol_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/list/node.rs", "active_symbol_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/list/node.rs", "disabled_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/list/node.rs", "empty_text_style"): "non-state visual part style",
    ("src/widgets/list/node.rs", "scroll_indicator_style"): "non-state visual part style",
    ("src/widgets/list/node.rs", "scrollbar_thumb_focus_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/list/node.rs", "scrollbar_thumb_style"): "non-state visual part style",
    ("src/widgets/list/node.rs", "scrollbar_track_style"): "non-state visual part style",
    ("src/widgets/list/node.rs", "selection_symbol_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/list/node.rs", "title_style"): "non-state visual part style",
    ("src/widgets/list/node.rs", "unfocused_selection_symbol_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/log_view/mod.rs", "debug_style"): "non-state visual part style",
    ("src/widgets/log_view/mod.rs", "empty_text_style"): "non-state visual part style",
    ("src/widgets/log_view/mod.rs", "error_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/log_view/mod.rs", "info_style"): "non-state visual part style",
    ("src/widgets/log_view/mod.rs", "scroll_indicator_style"): "non-state visual part style",
    ("src/widgets/log_view/mod.rs", "trace_style"): "non-state visual part style",
    ("src/widgets/log_view/mod.rs", "warn_style"): "non-state visual part style",
    ("src/widgets/modal.rs", "backdrop_style"): "non-state visual part style",
    ("src/widgets/modal.rs", "frame_style"): "non-state visual part style",
    ("src/widgets/modal.rs", "title_style"): "non-state visual part style",
    ("src/widgets/multi_select.rs", "active_symbol_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/multi_select.rs", "description_style"): "non-state visual part style",
    ("src/widgets/multi_select.rs", "disabled_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/multi_select.rs", "title_style"): "non-state visual part style",
    ("src/widgets/pagination.rs", "button_disabled_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/pagination.rs", "button_style"): "non-state visual part style",
    ("src/widgets/pagination.rs", "disabled_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/pagination.rs", "info_style"): "non-state visual part style",
    ("src/widgets/progress/mod.rs", "empty_style"): "non-state visual part style",
    ("src/widgets/progress/mod.rs", "filled_style"): "non-state visual part style",
    ("src/widgets/progress/mod.rs", "label_style"): "non-state visual part style",
    ("src/widgets/progress/mod.rs", "target_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/progress/node.rs", "empty_style"): "non-state visual part style",
    ("src/widgets/progress/node.rs", "filled_style"): "non-state visual part style",
    ("src/widgets/progress/node.rs", "label_style"): "non-state visual part style",
    ("src/widgets/progress/node.rs", "target_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/radio.rs", "checked_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/radio.rs", "disabled_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/radio.rs", "focus_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/radio.rs", "hover_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/radio.rs", "label_style"): "non-state visual part style",
    ("src/widgets/radio.rs", "unchecked_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/scroll_view/mod.rs", "scroll_indicator_style"): "non-state visual part style",
    ("src/widgets/scroll_view/node.rs", "h_scrollbar_thumb_focus_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/scroll_view/node.rs", "h_scrollbar_thumb_style"): "non-state visual part style",
    ("src/widgets/scroll_view/node.rs", "h_scrollbar_track_style"): "non-state visual part style",
    ("src/widgets/scroll_view/node.rs", "scroll_indicator_style"): "non-state visual part style",
    ("src/widgets/scroll_view/node.rs", "scrollbar_thumb_focus_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/scroll_view/node.rs", "scrollbar_thumb_style"): "non-state visual part style",
    ("src/widgets/scroll_view/node.rs", "scrollbar_track_style"): "non-state visual part style",
    ("src/widgets/search_palette/mod.rs", "active_description_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/search_palette/mod.rs", "active_item_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/search_palette/mod.rs", "description_style"): "non-state visual part style",
    ("src/widgets/search_palette/mod.rs", "focused_description_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/search_palette/mod.rs", "header_style"): "non-state visual part style",
    ("src/widgets/search_palette/render.rs", "header_style"): "non-state visual part style",
    ("src/widgets/search_palette/mod.rs", "input_divider_style"): "non-state visual part style",
    ("src/widgets/search_palette/mod.rs", "input_focus_content_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/search_palette/mod.rs", "input_focus_placeholder_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/search_palette/mod.rs", "input_focus_prefix_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/search_palette/mod.rs", "input_focus_suffix_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/search_palette/mod.rs", "input_placeholder_style"): "non-state visual part style",
    ("src/widgets/search_palette/mod.rs", "input_prefix_style"): "non-state visual part style",
    ("src/widgets/search_palette/mod.rs", "input_style"): "non-state visual part style",
    ("src/widgets/search_palette/mod.rs", "input_suffix_style"): "non-state visual part style",
    ("src/widgets/search_palette/mod.rs", "item_style"): "non-state visual part style",
    ("src/widgets/search_palette/mod.rs", "list_active_symbol_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/search_palette/mod.rs", "match_style"): "non-state visual part style",
    ("src/widgets/select/mod.rs", "button_disabled_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/select/mod.rs", "button_style"): "non-state visual part style",
    ("src/widgets/select/mod.rs", "button_suffix_style"): "non-state visual part style",
    ("src/widgets/select/mod.rs", "list_disabled_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/select/mod.rs", "list_title_style"): "non-state visual part style",
    ("src/widgets/slider/mod.rs", "filled_track_style"): "non-state visual part style",
    ("src/widgets/slider/mod.rs", "label_style"): "non-state visual part style",
    ("src/widgets/slider/mod.rs", "thumb_style"): "non-state visual part style",
    ("src/widgets/slider/node.rs", "filled_track_style"): "non-state visual part style",
    ("src/widgets/slider/node.rs", "label_style"): "non-state visual part style",
    ("src/widgets/slider/node.rs", "thumb_style"): "non-state visual part style",
    ("src/widgets/sparkline/mod.rs", "falling_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/sparkline/mod.rs", "flat_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/sparkline/mod.rs", "rising_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/sparkline/mod.rs", "turn_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/sparkline/node.rs", "falling_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/sparkline/node.rs", "flat_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/sparkline/node.rs", "rising_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/sparkline/node.rs", "turn_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/spinner/mod.rs", "label_style"): "non-state visual part style",
    ("src/widgets/spinner/node.rs", "label_style"): "non-state visual part style",
    ("src/widgets/splitter/mod.rs", "handle_active_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/splitter/mod.rs", "handle_hover_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/splitter/mod.rs", "handle_style"): "non-state visual part style",
    ("src/widgets/splitter/node.rs", "handle_active_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/splitter/node.rs", "handle_hover_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/splitter/node.rs", "handle_style"): "non-state visual part style",
    ("src/widgets/status_bar.rs", "center_style"): "non-state visual part style",
    ("src/widgets/status_bar.rs", "left_style"): "non-state visual part style",
    ("src/widgets/status_bar.rs", "loading_style"): "non-state visual part style",
    ("src/widgets/status_bar.rs", "right_style"): "non-state visual part style",
    ("src/widgets/table/mod.rs", "alternating_row_style"): "non-state visual part style",
    ("src/widgets/table/mod.rs", "disabled_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/table/mod.rs", "inspector_key_style"): "non-state visual part style",
    ("src/widgets/table/mod.rs", "inspector_section_style"): "non-state visual part style",
    ("src/widgets/table/mod.rs", "inspector_separator_style"): "non-state visual part style",
    ("src/widgets/table/mod.rs", "inspector_value_style"): "non-state visual part style",
    ("src/widgets/table/mod.rs", "scroll_indicator_style"): "non-state visual part style",
    ("src/widgets/table/mod.rs", "selection_symbol_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/table/node.rs", "alternating_row_style"): "non-state visual part style",
    ("src/widgets/table/node.rs", "disabled_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/table/node.rs", "inspector_key_style"): "non-state visual part style",
    ("src/widgets/table/node.rs", "inspector_section_style"): "non-state visual part style",
    ("src/widgets/table/node.rs", "inspector_separator_style"): "non-state visual part style",
    ("src/widgets/table/node.rs", "inspector_value_style"): "non-state visual part style",
    ("src/widgets/table/node.rs", "scroll_indicator_style"): "non-state visual part style",
    ("src/widgets/table/node.rs", "scrollbar_thumb_focus_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/table/node.rs", "scrollbar_thumb_style"): "non-state visual part style",
    ("src/widgets/table/node.rs", "scrollbar_track_style"): "non-state visual part style",
    ("src/widgets/table/node.rs", "selection_symbol_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/tabs/mod.rs", "disabled_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/tabs/node.rs", "disabled_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/terminal/mod_private.rs", "focus_content_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/terminal/mod_private.rs", "scrollbar_thumb_focus_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/terminal/mod_private.rs", "scrollbar_thumb_style"): "non-state visual part style",
    ("src/widgets/terminal/mod_private.rs", "scrollbar_track_style"): "non-state visual part style",
    ("src/widgets/terminal/node.rs", "focus_content_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/terminal/node.rs", "scrollbar_thumb_focus_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/terminal/node.rs", "scrollbar_thumb_style"): "non-state visual part style",
    ("src/widgets/terminal/node.rs", "scrollbar_track_style"): "non-state visual part style",
    ("src/widgets/text_area/mod.rs", "disabled_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/text_area/mod.rs", "focus_content_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/text_area/mod.rs", "focus_placeholder_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/text_area/mod.rs", "image_placeholder_focus_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/text_area/mod.rs", "image_placeholder_hover_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/text_area/mod.rs", "image_placeholder_style"): "non-state visual part style",
    ("src/widgets/text_area/mod.rs", "line_number_style"): "non-state visual part style",
    ("src/widgets/text_area/mod.rs", "placeholder_style"): "non-state visual part style",
    ("src/widgets/text_area/mod.rs", "split_wrap_padding_style"): "split-wrap padding visual part style",
    ("src/widgets/text_area/mod.rs", "split_wrap_padding_gutter_style"): "non-state visual part style",
    ("src/widgets/text_area/node.rs", "disabled_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/text_area/node.rs", "focus_content_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/text_area/node.rs", "focus_placeholder_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/text_area/node.rs", "image_placeholder_focus_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/text_area/node.rs", "image_placeholder_hover_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/text_area/node.rs", "image_placeholder_style"): "non-state visual part style",
    ("src/widgets/text_area/node.rs", "line_number_style"): "non-state visual part style",
    ("src/widgets/text_area/node.rs", "placeholder_style"): "non-state visual part style",
    ("src/widgets/text_area/node.rs", "scrollbar_thumb_focus_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/text_area/node.rs", "scrollbar_thumb_style"): "non-state visual part style",
    ("src/widgets/text_area/node.rs", "scrollbar_track_style"): "non-state visual part style",
    ("src/widgets/text_area/node.rs", "split_wrap_padding_style"): "split-wrap padding visual part style",
    ("src/widgets/text_area/node.rs", "split_wrap_padding_gutter_style"): "non-state visual part style",
    ("src/widgets/text_area/sentinel.rs", "focus_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/text_area/sentinel.rs", "hover_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/toast/mod.rs", "frame_style"): "non-state visual part style",
    ("src/widgets/toast/mod.rs", "message_style"): "non-state visual part style",
    ("src/widgets/toast/mod.rs", "title_style"): "non-state visual part style",
    ("src/widgets/tooltip.rs", "container_style"): "non-state visual part style",
    ("src/widgets/tooltip.rs", "text_style"): "non-state visual part style",
    ("src/widgets/tree/types.rs", "empty_text_style"): "non-state visual part style",
    ("src/widgets/tree/types.rs", "icon_style"): "non-state visual part style",
    ("src/widgets/tree/types.rs", "indent_guide_style"): "non-state visual part style",
    ("src/widgets/tree/types.rs", "scroll_indicator_style"): "non-state visual part style",
    ("src/widgets/tree/types.rs", "selection_symbol_style"): "legacy raw state style; TODO migrate to StyleSlot",
    ("src/widgets/tree/types.rs", "unfocused_selection_symbol_style"): "legacy raw state style; TODO migrate to StyleSlot",
}

FIELD_RE = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?"
    r"(?P<field>[A-Za-z_][A-Za-z0-9_]*_style)\s*:\s*"
    r"(?P<type>(?:Option\s*<\s*)?(?:crate::style::)?Style\s*>?)\s*,?\s*$"
)

STRUCT_RE = re.compile(r"\bstruct\s+[A-Za-z_][A-Za-z0-9_]*\b")

BARE_STYLE_TYPES = {
    "Style",
    "crate::style::Style",
    "Option<Style>",
    "Option<crate::style::Style>",
}


@dataclass(frozen=True)
class Violation:
    path: Path
    line: int
    field: str
    style_type: str


def read(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except OSError as exc:
        raise SystemExit(f"error: failed to read {path.relative_to(ROOT)}: {exc}") from exc


def strip_comments(text: str) -> str:
    text = re.sub(r"//.*", "", text)
    return re.sub(r"/\*.*?\*/", lambda match: "\n" * match.group(0).count("\n"), text, flags=re.DOTALL)


def normalize_type(style_type: str) -> str:
    return re.sub(r"\s+", "", style_type)


def find_matching_brace(text: str, open_brace_index: int) -> int:
    depth = 0
    for index in range(open_brace_index, len(text)):
        char = text[index]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return index
    raise ValueError("unclosed struct body")


def braced_struct_line_ranges(text: str) -> list[tuple[int, int]]:
    ranges: list[tuple[int, int]] = []
    for match in STRUCT_RE.finditer(text):
        cursor = match.end()
        while cursor < len(text) and text[cursor] not in "{;":
            cursor += 1
        if cursor >= len(text) or text[cursor] != "{":
            continue

        try:
            close_brace = find_matching_brace(text, cursor)
        except ValueError as exc:
            raise SystemExit(f"error: unclosed braced struct near byte {match.start()}") from exc

        start_line = text.count("\n", 0, cursor) + 1
        end_line = text.count("\n", 0, close_brace) + 1
        ranges.append((start_line, end_line))
    return ranges


def find_raw_style_fields() -> list[Violation]:
    fields: list[Violation] = []
    for path in sorted(WIDGETS.glob("**/*.rs")):
        rel = path.relative_to(ROOT).as_posix()
        text = strip_comments(read(path))
        struct_ranges = braced_struct_line_ranges(text)
        struct_lines = {
            line
            for start_line, end_line in struct_ranges
            for line in range(start_line + 1, end_line)
        }
        for index, line in enumerate(text.splitlines(), start=1):
            if index not in struct_lines:
                continue
            match = FIELD_RE.match(line)
            if not match:
                continue
            field = match.group("field")
            if field == "style":
                continue
            style_type = normalize_type(match.group("type"))
            if style_type not in BARE_STYLE_TYPES:
                continue
            fields.append(Violation(path=path, line=index, field=field, style_type=style_type))
    return fields


def main() -> int:
    raw_style_fields = find_raw_style_fields()
    missing_reasons = sorted(key for key, reason in ALLOWLIST.items() if not reason.strip())
    if missing_reasons:
        print("Widget style-slot allowlist entries require reasons:\n", file=sys.stderr)
        for rel, field in missing_reasons:
            print(f"{rel}: field `{field}` has an empty allowlist reason", file=sys.stderr)
        return 1

    used_allowlist = {
        (violation.path.relative_to(ROOT).as_posix(), violation.field)
        for violation in raw_style_fields
        if (violation.path.relative_to(ROOT).as_posix(), violation.field) in ALLOWLIST
    }
    violations = [
        violation
        for violation in raw_style_fields
        if (violation.path.relative_to(ROOT).as_posix(), violation.field) not in ALLOWLIST
    ]
    stale_allowlist = sorted(set(ALLOWLIST) - used_allowlist)

    if stale_allowlist:
        print("Widget style-slot allowlist has stale entries:\n", file=sys.stderr)
        for rel, field in stale_allowlist:
            print(f"{rel}: field `{field}` is allowlisted but was not found", file=sys.stderr)
        print("\nRemove stale entries from ALLOWLIST.", file=sys.stderr)
        return 1

    if not violations:
        print(f"widget style-slot guard OK ({len(used_allowlist)} raw Style fields allowlisted).")
        return 0

    print("Widget style-slot guard failed:\n", file=sys.stderr)
    for violation in violations:
        rel = violation.path.relative_to(ROOT).as_posix()
        print(
            f"{rel}:{violation.line}: struct field `{violation.field}: {violation.style_type}` "
            "uses bare Style",
            file=sys.stderr,
        )
    print(
        "\nStruct state overlay fields ending `_style` should be `StyleSlot` so widgets can "
        "replace, extend, or inherit theme roles. Use `StyleSlot` for new "
        "hover/focus/active/selection styles, or add an exact ALLOWLIST entry "
        "in scripts/check-widget-style-slots.py with a reason if this is a "
        "legitimate non-state style or a documented legacy exception.",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
