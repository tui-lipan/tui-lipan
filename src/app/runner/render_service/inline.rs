use crossterm::{
    cursor::MoveTo,
    execute,
    style::Print,
    terminal::{Clear, ClearType},
};
use ratatui::backend::Backend;
use ratatui::layout::Position;
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Widget};
use std::cell::{Cell as StdCell, RefCell};

use crate::Result;
use crate::backend::ratatui_backend::{RenderContext, render};
use crate::core::component::Component;
use crate::core::element::Element;
use crate::core::node::NodeTree;
use crate::core::runtime_env::TranscriptEntry;
use crate::layout::measure::min_size_constrained;
use crate::layout::reconcile::reconcile_with_overlays_mode;
use crate::style::Rect;

use super::AppRunner;

pub(super) fn reanchor_inline_viewport_to_top<B: Backend>(
    terminal: &mut ratatui::Terminal<B>,
) -> std::result::Result<(), B::Error> {
    let size = terminal.size()?;
    terminal.set_cursor_position(Position::ORIGIN)?;
    terminal.resize(size.into())?;
    Ok(())
}

pub(super) fn clear_inline_transcript_surface<B: Backend>(
    terminal: &mut ratatui::Terminal<B>,
) -> std::result::Result<(), B::Error> {
    terminal.clear()?;
    reanchor_inline_viewport_to_top(terminal)?;
    Ok(())
}

pub(super) fn host_terminal_erase_scrollback_and_visible(
    terminal: &mut crate::backend::ratatui_backend::Terminal,
) -> std::io::Result<()> {
    let out = terminal.backend_mut();
    std::io::Write::flush(out)?;
    execute!(out, Print("\x1b[3J"), Clear(ClearType::All), MoveTo(0, 0),)?;
    std::io::Write::flush(out)
}

impl<C: Component> AppRunner<C> {
    pub(super) fn flush_inline_inserts(
        &mut self,
        terminal: &mut crate::backend::ratatui_backend::Terminal,
    ) -> Result<()> {
        if !self.surface.is_inline() {
            self.core.clear_pending_transcript_entries();
            return Ok(());
        }

        const MAX_INSERT_LINES_PER_CALL: usize = 256;

        let mut pending = self.core.take_pending_transcript_entries();
        while let Some(entry) = pending.pop_front() {
            match entry {
                TranscriptEntry::Lines(batch) => {
                    if batch.is_empty() {
                        continue;
                    }

                    self.flush_inline_transcript_lines(
                        terminal,
                        &batch,
                        MAX_INSERT_LINES_PER_CALL,
                    )?;
                }
                TranscriptEntry::Element(element) => {
                    self.flush_inline_element_commit(terminal, *element)?;
                }
            }
        }

        Ok(())
    }

    pub(crate) fn replay_inline_transcript_document(
        &mut self,
        terminal: &mut crate::backend::ratatui_backend::Terminal,
        include_live_viewport: bool,
    ) -> Result<()> {
        if !self.surface.is_transcript() {
            return Ok(());
        }

        self.core.clear_pending_transcript_entries();
        let document = self.core.transcript_replay_document(include_live_viewport);
        if document.is_empty() {
            return Ok(());
        }

        clear_inline_transcript_surface(terminal)?;
        self.last_frame_snapshot = None;
        self.scroll_diff_snapshot = None;

        self.flush_transcript_document_entries(terminal, document)
    }

    pub(super) fn flush_transcript_document_entries(
        &mut self,
        terminal: &mut crate::backend::ratatui_backend::Terminal,
        document: Vec<TranscriptEntry>,
    ) -> Result<()> {
        if document.is_empty() {
            self.core.clear_pending_transcript_entries();
            return Ok(());
        }

        const MAX_INSERT_LINES_PER_CALL: usize = 256;

        for entry in document {
            match entry {
                TranscriptEntry::Lines(lines) => {
                    self.flush_inline_transcript_lines(
                        terminal,
                        &lines,
                        MAX_INSERT_LINES_PER_CALL,
                    )?;
                }
                TranscriptEntry::Element(element) => {
                    self.flush_inline_element_commit(terminal, *element)?;
                }
            }
        }

        self.core.clear_pending_transcript_entries();
        Ok(())
    }

    pub(crate) fn clear_inline_transcript_surface_for_exit(
        &mut self,
        terminal: &mut crate::backend::ratatui_backend::Terminal,
    ) -> Result<()> {
        if !self.surface.is_transcript() {
            return Ok(());
        }

        clear_inline_transcript_surface(terminal)?;
        self.last_frame_snapshot = None;
        self.scroll_diff_snapshot = None;
        Ok(())
    }

    fn flush_inline_transcript_lines(
        &self,
        terminal: &mut crate::backend::ratatui_backend::Terminal,
        lines: &[crate::style::RichText],
        max_chunk_lines: usize,
    ) -> Result<()> {
        let width = terminal.size()?.width.max(1);
        for chunk in lines.chunks(max_chunk_lines.max(1)) {
            let wrapped_lines = wrap_transcript_visual_lines(chunk, width);
            let height = wrapped_lines.len().min(u16::MAX as usize) as u16;
            if height == 0 {
                continue;
            }

            terminal.insert_before(height, |buf| {
                Paragraph::new(wrapped_lines).render(buf.area, buf);
            })?;
        }

        Ok(())
    }

    pub(super) fn render_inline_element_commit_buffer(
        &mut self,
        width: u16,
        element: Element,
    ) -> Option<ratatui::buffer::Buffer> {
        let height = min_size_constrained(&element, Some(width), None).1;
        if height == 0 {
            return None;
        }

        let bounds = Rect {
            x: 0,
            y: 0,
            w: width,
            h: height,
        };

        let mut tree = NodeTree::new();
        reconcile_with_overlays_mode(&mut tree, &element, bounds, None, &[], true);
        let join_index = crate::backend::ratatui_backend::render::build_join_index(&tree);
        let scrollbar_metrics_cache = RefCell::new(Default::default());
        let overlay_bg_snapshot = RefCell::new(Vec::new());
        let dnd_snapshot_cells = RefCell::new(None);
        let cursor_position = StdCell::new(None);
        let ctx = RenderContext {
            tree: &tree,
            focused: None,
            hovered: None,
            mouse_pos: None,
            suppress_pointer_item_hover_nodes: None,
            blink_visible: true,
            effect_phase: 0,
            images_enabled: false,
            contrast_policy: self.contrast_policy,
            read_only_selection: None,
            scrollbar_metrics_cache: &scrollbar_metrics_cache,
            overlay_bg_snapshot: &overlay_bg_snapshot,
            join_index: &join_index,
            cursor_position: &cursor_position,
            terminal_bg: self
                .terminal_bg
                .map(crate::backend::ratatui_backend::common::to_ratatui_color),
            drag_preview_label: None,
            drag_preview_at_mouse: false,
            drag_preview_snapshot_rect: None,
            dnd_snapshot_cells: &dnd_snapshot_cells,
            drag_preview_max_width: None,
            drag_preview_max_height: None,
            drop_slot_source_preview_rect: None,
            paint_glyph_caches: Some(self.paint_glyph_caches.clone()),
            copy_feedback: None,
            copy_feedback_style: self.clipboard_config.copy_feedback_style,
        };

        let scratch = self
            .inline_commit_scratch
            .get_or_insert_with(|| new_inline_commit_scratch(width, height));

        let current_area = scratch.current_buffer_mut().area;
        if current_area.width != width || current_area.height != height {
            // `Terminal::resize` on a fixed viewport queries the host terminal
            // size (`backend.size()` in `clear_fixed_viewport`), which fails
            // with EAGAIN on tty-less runners. Recreating the scratch stays
            // fully in-memory: `with_options` never touches the backend for
            // `Viewport::Fixed`.
            *scratch = new_inline_commit_scratch(width, height);
        }

        {
            let mut frame = scratch.get_frame();
            frame.buffer_mut().reset();
            render(&mut frame, &ctx);
        }

        Some(scratch.current_buffer_mut().clone())
    }

    pub(crate) fn flush_inline_element_commit(
        &mut self,
        terminal: &mut crate::backend::ratatui_backend::Terminal,
        element: Element,
    ) -> Result<()> {
        // Match `content_bounds` inline width: last column is reserved so the cursor
        // never hits the terminal's wrap edge (see `AppRunner::content_bounds`).
        let width = terminal.size()?.width.saturating_sub(1).max(1);
        let Some(rendered) = self.render_inline_element_commit_buffer(width, element) else {
            return Ok(());
        };
        let height = rendered.area.height;

        terminal.insert_before(height, |buf| {
            for y in 0..height {
                for x in 0..width {
                    if let Some(dst) = buf.cell_mut((x, y)) {
                        *dst = rendered[(x, y)].clone();
                    }
                }
            }
        })?;

        Ok(())
    }

    /// Measure the live element's natural height for `InlineHeight::Auto` and
    /// resize the inline viewport to match. Returns the new content bounds
    /// when they differ from `bounds`, so the caller can re-reconcile at the
    /// final size.
    ///
    /// The layout bounds stay at the content's natural height even when it
    /// exceeds the terminal; only the on-screen viewport is clamped (to the
    /// `max` cap and the terminal height), showing the top of the layout and
    /// clipping the rest at draw time.
    pub(super) fn sync_inline_auto_height(
        &mut self,
        terminal: &mut crate::backend::ratatui_backend::Terminal,
        bounds: Rect,
    ) -> Result<Option<Rect>> {
        let Some(max_rows) = self.surface.auto_inline_height() else {
            return Ok(None);
        };
        if self.surface.is_transcript() && self.surface.inline.transcript_expanded {
            // The expanded-transcript flow sizes the viewport from the full
            // replay document instead (see `render_expanded_transcript`).
            return Ok(None);
        }
        let Some(element) = self.core.cached_expanded_element.as_ref() else {
            return Ok(None);
        };

        let natural = min_size_constrained(element, Some(bounds.w), None).1.max(1);
        self.surface.inline.auto_height_resolved = natural;

        let size = terminal.size()?;
        let viewport_rows = max_rows
            .map_or(natural, |cap| natural.min(cap))
            .min(size.height)
            .max(1);
        if terminal.get_frame().area().height != viewport_rows {
            self.resize_inline_viewport(terminal, viewport_rows)?;
        }

        let new_bounds = self.content_bounds(size.width, size.height);
        Ok((new_bounds != bounds).then_some(new_bounds))
    }

    /// Recreate the inline terminal with a new viewport height, anchored at
    /// the current viewport top. Growing near the bottom of the screen scrolls
    /// host content up (via ratatui's `compute_inline_size`).
    fn resize_inline_viewport(
        &mut self,
        terminal: &mut crate::backend::ratatui_backend::Terminal,
        rows: u16,
    ) -> Result<()> {
        let viewport = terminal.get_frame().area();
        // The recreated terminal anchors its viewport to the cursor row.
        execute!(terminal.backend_mut(), MoveTo(0, viewport.y))?;
        *terminal = crate::backend::ratatui_backend::create_inline_terminal(rows)?;
        // The new terminal's back buffer is empty, so the next draw skips
        // cells that are blank in the frame. Erase from the new viewport top
        // to the end of the display (old viewport rows always sit at or below
        // it) so stale content cannot show through those skipped cells.
        terminal.clear()?;
        // Seed last_terminal_size so the size-change guard in
        // draw_current_tree does not treat the recreation as a host resize.
        if let Ok(size) = terminal.size() {
            self.surface.inline.last_terminal_size = (size.width, size.height);
        }
        self.last_frame_snapshot = None;
        self.scroll_diff_snapshot = None;
        self.last_scroll_frames.clear();
        Ok(())
    }

    pub(super) fn capture_inline_cursor_offset(
        &mut self,
        frame_area: ratatui::layout::Rect,
        cursor_position: Option<Position>,
    ) {
        if !self.surface.is_inline() || frame_area.height == 0 {
            self.surface.inline.inline_cursor_offset = 0;
            return;
        }

        let max_offset = frame_area.height.saturating_sub(1);

        if !self.focused_node_has_cursor_anchor() {
            self.surface.inline.inline_cursor_offset =
                self.surface.inline.inline_cursor_offset.min(max_offset);
            return;
        }

        // When the renderer does not request a cursor this frame (for example
        // blink-off phase), ratatui leaves the backend cursor at the last
        // painted cell. Sampling that position would corrupt the inline anchor
        // offset and break resize/reflow placement. Keep the previous offset
        // until a real caret is rendered again.
        if !self.focused_node_requests_cursor() {
            self.surface.inline.inline_cursor_offset =
                self.surface.inline.inline_cursor_offset.min(max_offset);
            return;
        }

        if let Some(pos) = cursor_position {
            let min_y = frame_area.y;
            let max_y = frame_area.y.saturating_add(max_offset);
            let clamped_y = pos.y.clamp(min_y, max_y);
            self.surface.inline.inline_cursor_offset = clamped_y.saturating_sub(frame_area.y);
        } else {
            self.surface.inline.inline_cursor_offset =
                self.surface.inline.inline_cursor_offset.min(max_offset);
        }
    }

    pub(super) fn stabilize_inline_resize_anchor(
        &mut self,
        terminal: &mut crate::backend::ratatui_backend::Terminal,
    ) -> Result<()> {
        if !self.surface.is_inline() || self.focused_node_requests_cursor() {
            return Ok(());
        }

        let max_offset = self
            .surface
            .inline
            .viewport_metrics
            .height
            .saturating_sub(1);
        let offset = self.surface.inline.inline_cursor_offset.min(max_offset);

        terminal.set_cursor_position(ratatui::layout::Position {
            x: self.surface.inline.viewport_metrics.x,
            y: self
                .surface
                .inline
                .viewport_metrics
                .y
                .saturating_add(offset),
        })?;
        Ok(())
    }
}

pub(super) fn wrap_transcript_visual_lines(
    lines: &[crate::style::RichText],
    width: u16,
) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut visual_lines = Vec::new();

    for line in lines {
        for logical_line in crate::widgets::internal::split_spans_on_newlines(&line.spans) {
            for wrapped in crate::utils::text::wrap_spans_for_budgets(&logical_line, width, width) {
                let rat_spans = wrapped
                    .into_iter()
                    .map(|span| {
                        ratatui::text::Span::styled(
                            span.content.to_string(),
                            crate::backend::ratatui_backend::common::to_ratatui_style(span.style),
                        )
                    })
                    .collect::<Vec<_>>();
                visual_lines.push(Line::from(rat_spans));
            }
        }
    }

    if visual_lines.is_empty() {
        visual_lines.push(Line::default());
    }

    visual_lines
}

/// Builds the in-memory scratch terminal used for inline transcript element
/// commits. Uses a `Vec<u8>` writer and a `Viewport::Fixed`, so construction
/// never queries the host terminal and works on tty-less runners.
fn new_inline_commit_scratch(
    width: u16,
    height: u16,
) -> ratatui::Terminal<ratatui::backend::CrosstermBackend<Vec<u8>>> {
    let backend = ratatui::backend::CrosstermBackend::new(Vec::new());
    ratatui::Terminal::with_options(
        backend,
        ratatui::TerminalOptions {
            viewport: ratatui::Viewport::Fixed(ratatui::layout::Rect::new(0, 0, width, height)),
        },
    )
    .expect("inline commit scratch terminal should init")
}
