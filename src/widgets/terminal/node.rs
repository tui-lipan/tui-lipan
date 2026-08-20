use std::sync::Arc;

use crate::callback::{Callback, KeyHandler};
use crate::core::node::{
    NodeId, ScrollbarZone, ScrollbarZonesParams, WidgetNode, compute_scrollbar_zones,
};
use crate::style::{
    BorderStyle, CaretShape, Color, Padding, Rect, ScrollbarVariant, Span, Style, StyleSlot, Theme,
    ThemeRole,
};
use crate::utils::hints::HintSpan;
use crate::widgets::ScrollEvent;

use super::events::{
    MouseModeState, TerminalInputEvent, TerminalKeyModes, TerminalLinkEvent,
    TerminalPasteShortcutBehavior,
};
#[cfg(feature = "terminal-images")]
use super::graphics::TerminalImagePlacement;
use super::layout::terminal_content_layout;
use super::screen::{
    TerminalDecoration, TerminalHyperlink, TerminalRenderSnapshot, TerminalScreenHandle,
};
use super::selection::{
    ScrollbackLineage, TerminalSelection, TerminalSelectionEvent, rebase_selection,
};

/// Runtime node for terminal rendering.
#[derive(Clone)]
pub(crate) struct TerminalNode {
    pub text: Arc<str>,
    pub lines: Arc<[Vec<Span>]>,
    pub wrapped_rows: Arc<[bool]>,
    pub hyperlinks: Arc<[TerminalHyperlink]>,
    pub link_hover: Option<TerminalLinkHover>,
    pub cursor_row: u16,
    pub cursor_col: u16,
    pub cursor_visible: bool,
    pub cursor_shape: CaretShape,
    pub cursor_blinking: bool,
    pub caret_color: Option<Color>,
    pub selection: Option<TerminalSelection>,
    pub selection_controlled: bool,
    pub lineage: ScrollbackLineage,
    pub selection_style: StyleSlot,
    pub mouse_mode: MouseModeState,
    pub key_modes: TerminalKeyModes,
    /// Images to paint over the content rect, back to front.
    #[cfg(feature = "terminal-images")]
    pub images: Arc<[TerminalImagePlacement]>,
    /// Present when the app handed over a live screen rather than a snapshot. The runtime refreshes
    /// the fields above from it before every draw, which is what lets terminal output be a repaint.
    pub screen: Option<TerminalScreenHandle>,
    /// Overlays re-applied on each refresh from `screen`.
    pub decorations: Arc<[TerminalDecoration]>,
    /// Sequence of the snapshot the fields above were last filled from, so a refresh that finds the
    /// screen unmoved does nothing.
    pub live_sequence: Option<u64>,
    /// Cursor visibility the app asked for, ANDed with what the live screen reports. A pane that is
    /// exited or wearing hint labels hides its caret no matter what the child program wants.
    pub show_cursor_requested: bool,
    pub paste_shortcut_behavior: TerminalPasteShortcutBehavior,
    pub on_selection: Option<Callback<TerminalSelectionEvent>>,
    pub on_mouse_forward: Option<Callback<Vec<u8>>>,
    pub link_activation_mods: crate::core::event::KeyMods,
    pub link_hover_style: StyleSlot,
    pub on_link_activate: Option<Callback<TerminalLinkEvent>>,
    pub style: Style,
    pub hover_style: StyleSlot,
    pub focus_style: StyleSlot,
    pub focus_content_style: Style,
    pub border: bool,
    pub border_style: BorderStyle,
    pub padding: Padding,
    pub scrollback_offset: usize,
    pub snapshot_scrollback_offset: usize,
    pub total_scrollback_rows: usize,
    pub viewport_rows: usize,
    pub viewport_cols: usize,
    pub scroll_wheel: bool,
    pub scroll_override: Option<usize>,
    pub scrollbar: bool,
    pub scrollbar_variant: ScrollbarVariant,
    pub scrollbar_gap: u16,
    pub scrollbar_thumb: Option<char>,
    pub scrollbar_thumb_style: Option<Style>,
    pub scrollbar_thumb_focus_style: Option<Style>,
    pub scrollbar_track_style: Option<Style>,
    pub on_scroll: Option<Callback<ScrollEvent>>,
    pub on_scroll_to: Option<Callback<usize>>,
    pub focusable: bool,
    pub tab_stop: bool,
    pub on_focus: Option<Callback<()>>,
    pub on_blur: Option<Callback<()>>,
    pub on_key: Option<KeyHandler>,
    pub on_input: Option<Callback<TerminalInputEvent>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TerminalLinkHover {
    pub uri: Arc<str>,
    pub spans: Arc<[HintSpan]>,
}

impl TerminalNode {
    /// Pull the live screen's current snapshot into this node, if it has moved since the last pull.
    ///
    /// Returns whether anything changed, so a caller can tell a repaint is warranted. Terminal
    /// output is node-local state in exactly the way a scroll offset is: the element that produced
    /// this node says nothing about it, so refreshing here — rather than by re-running `view()` —
    /// is what keeps a streaming pane from rebuilding the whole window on every chunk.
    pub(crate) fn refresh_from_live_screen(&mut self) -> bool {
        let Some(screen) = self.screen.clone() else {
            return false;
        };
        let snapshot = screen.snapshot();
        let snapshot = if self.decorations.is_empty() {
            snapshot
        } else {
            snapshot.decorated(&self.decorations)
        };
        if self.live_sequence == Some(snapshot.sequence) {
            return false;
        }
        self.live_sequence = Some(snapshot.sequence);
        self.apply_snapshot(&snapshot);
        true
    }

    /// Fill the snapshot-derived fields, keeping the scroll rules `reconcile_terminal` establishes:
    /// a moved snapshot offset is authoritative and retires an input-driven override.
    fn apply_snapshot(&mut self, snapshot: &TerminalRenderSnapshot) {
        let to = ScrollbackLineage {
            evicted_lines: snapshot.evicted_lines,
            history_epoch: snapshot.history_epoch,
        };
        self.selection = rebase_selection(self.selection, self.lineage, to);
        self.lineage = to;
        if self.text != snapshot.text
            || self.wrapped_rows != snapshot.wrapped_rows
            || self.hyperlinks != snapshot.hyperlinks
        {
            self.link_hover = None;
        }
        self.text = snapshot.text.clone();
        self.lines = snapshot.color_lines.clone();
        self.wrapped_rows = snapshot.wrapped_rows.clone();
        self.hyperlinks = snapshot.hyperlinks.clone();
        self.cursor_row = snapshot.cursor_row;
        self.cursor_col = snapshot.cursor_col;
        self.cursor_visible = snapshot.cursor_visible && self.show_cursor_requested;
        self.cursor_shape = snapshot.cursor_shape;
        self.cursor_blinking = snapshot.cursor_blinking;
        self.mouse_mode = snapshot.mouse_mode;
        self.key_modes = snapshot.key_modes;
        #[cfg(feature = "terminal-images")]
        {
            self.images = snapshot.images.clone();
        }
        self.total_scrollback_rows = snapshot.total_scrollback_rows;
        if snapshot.scrollback_offset != self.snapshot_scrollback_offset {
            self.snapshot_scrollback_offset = snapshot.scrollback_offset;
            self.scroll_override = None;
        }
        self.scrollback_offset = self.scroll_override.unwrap_or(snapshot.scrollback_offset);
    }
}

pub(crate) fn apply_terminal_selection_input(
    selection: &mut Option<TerminalSelection>,
    controlled: bool,
    proposed: Option<TerminalSelection>,
) {
    if !controlled {
        *selection = proposed;
    }
}

impl WidgetNode for TerminalNode {
    fn is_focusable(&self) -> bool {
        self.focusable
    }

    fn is_tab_stop(&self) -> bool {
        self.focusable && self.tab_stop
    }

    fn on_focus_callback(&self) -> Option<&Callback<()>> {
        self.on_focus.as_ref()
    }

    fn on_blur_callback(&self) -> Option<&Callback<()>> {
        self.on_blur.as_ref()
    }

    fn has_on_click(&self) -> bool {
        self.on_scroll.is_some()
            || self.on_scroll_to.is_some()
            || self.scrollbar
            || self.scroll_wheel
    }

    fn is_hoverable(&self) -> bool {
        self.hover_style.has_explicit_style()
    }

    fn is_hoverable_for_theme(&self, theme: &Theme) -> bool {
        self.hover_style.resolves_non_empty(theme, ThemeRole::Hover)
    }

    fn scrollbar_zones(
        &self,
        id: NodeId,
        rect: Rect,
        parent_border_x: Option<i16>,
        _parent_border_y: Option<i16>,
    ) -> Vec<ScrollbarZone> {
        if !self.scrollbar {
            return Vec::new();
        }

        let inner = rect.inner(self.border, self.padding);
        if inner.w == 0 || inner.h == 0 {
            return Vec::new();
        }

        let layout = terminal_content_layout(
            inner,
            self.border,
            self.scrollbar,
            self.scrollbar_variant,
            self.scrollbar_gap,
            self.total_scrollback_rows,
            parent_border_x.is_some(),
        );
        if !layout.scrollbar_visible {
            return Vec::new();
        }

        compute_scrollbar_zones(ScrollbarZonesParams {
            id,
            rect,
            inner,
            border: self.border,
            scrollbar: self.scrollbar,
            scrollbar_variant: self.scrollbar_variant,
            scrollbar_gap: self.scrollbar_gap,
            h_scrollbar: false,
            h_scrollbar_variant: ScrollbarVariant::default(),
            content_x: inner.x,
            content_width: inner.w,
            max_content_width: 0,
            wrap: false,
            parent_border_x,
            parent_border_y: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::terminal::selection::TerminalPos;

    fn selection(col: usize) -> TerminalSelection {
        TerminalSelection {
            anchor: TerminalPos { line: 1, col },
            cursor: TerminalPos { line: 1, col },
        }
    }

    #[test]
    fn controlled_selection_ignores_mouse_selection_input() {
        let original = selection(4);
        let mut current = Some(original);

        apply_terminal_selection_input(&mut current, true, None);
        assert_eq!(current, Some(original));

        apply_terminal_selection_input(&mut current, true, Some(selection(8)));
        assert_eq!(current, Some(original));
    }

    #[test]
    fn uncontrolled_selection_accepts_mouse_selection_input() {
        let mut current = Some(selection(4));
        apply_terminal_selection_input(&mut current, false, None);
        assert_eq!(current, None);
    }
}
