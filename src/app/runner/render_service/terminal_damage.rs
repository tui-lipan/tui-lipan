//! Repainting a frame from terminal damage alone.
//!
//! Planning is separated from drawing so eligibility can be asserted without rendering anything:
//! a test can hand this a tree and a refresh result and check *why* a case was rejected. `None`
//! is always the safe answer, and means the caller does an ordinary paint over the tree it has
//! already refreshed.
//!
//! The draw is deliberately not a second renderer. It runs the ordinary tree walk through the
//! production render context, clipped to one row, so a patched row is by construction the same
//! cells a full paint would have produced there.

use ratatui::backend::Backend;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect as RatatuiRect};
use std::cell::Cell as StdCell;

use crate::app::AppRunner;
use crate::backend::ratatui_backend::{RenderContext, render_regions};
use crate::core::component::Component;
use crate::core::node::{LiveTerminalRefresh, NodeId, NodeKind};
use crate::widgets::internal::TerminalDamage;

/// Why a frame could not be repainted from damage. Recorded rather than returned as a bare `None`
/// so the reason is testable and shows up in debug logs instead of being inferred.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DamageRejection {
    /// No live terminal reported anything.
    NothingMoved,
    /// More than one terminal moved. Supporting several is possible and not yet worth it.
    SeveralTerminalsMoved,
    /// The terminal reported `Full` - a resize, a screen swap, decorations.
    FullDamage,
    /// The node backing the damage is gone or is no longer a terminal.
    NodeMissing,
    /// There is no retained frame to patch, or it does not match the current geometry.
    NoMatchingRetainedFrame,
    /// An overlay, devtools panel, drag preview or inline surface composites over the tree, so a
    /// terminal row is not the only thing that could occupy those cells.
    CompositeSurface,
    /// The terminal is showing scrollback rather than the live viewport.
    ScrolledBack,
    /// The terminal carries images, which are not painted from the cell grid.
    #[cfg_attr(not(feature = "terminal-images"), allow(dead_code))]
    HasImages,
    /// The terminal draws a border, so which physical row a viewport row lands on depends on
    /// which borders the clip leaves visible. Padding is a fixed offset and stays eligible.
    BorderedTerminal,
    /// Every damaged row falls outside the frame, so there is nothing to repaint.
    NothingVisible,
}

/// One terminal's damaged rows, resolved against the node that will paint them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TerminalDamagePlan {
    /// The terminal node the rows came from. The draw never needs it - it paints screen rows, not
    /// a node - but a test asserting *which* terminal was accepted does.
    #[cfg_attr(not(test), allow(dead_code))]
    pub node: NodeId,
    /// Physical frame rows this repaint has to paint, ascending, each inside the frame: every row
    /// the terminal moved, plus the row the focused widget's caret sits on.
    ///
    /// Resolved here rather than in the draw: the mapping from a viewport row to a screen row is
    /// geometry, and geometry is what the planner is for. The draw only paints rows.
    pub rows: Vec<u16>,
}

impl<C> AppRunner<C>
where
    C: Component + 'static,
{
    /// Whether this frame can be repainted from terminal damage, and if not, why.
    ///
    /// Pure with respect to the runner: it reads the refresh result, the tree and the retained
    /// frame, and mutates nothing. Damage has already been consumed by the refresh that produced
    /// `refresh`, so rejecting here costs nothing but a full repaint.
    pub(crate) fn plan_terminal_damage(
        &self,
        refresh: &LiveTerminalRefresh,
        frame_area: ratatui::layout::Rect,
    ) -> Result<TerminalDamagePlan, DamageRejection> {
        if self.surface.is_inline() || !self.core.tree.overlay_roots().is_empty() {
            return Err(DamageRejection::CompositeSurface);
        }
        #[cfg(feature = "devtools")]
        if self.devtools_state.borrow().visible {
            return Err(DamageRejection::CompositeSurface);
        }
        if !matches!(self.drag.active, crate::app::runner::ActiveDrag::None) {
            return Err(DamageRejection::CompositeSurface);
        }
        // The inspector outline is drawn over the finished frame by `render`, and `render_regions`
        // has no equivalent. A repainted row would erase the part of the outline crossing it.
        if self.highlight().is_some() {
            return Err(DamageRejection::CompositeSurface);
        }

        let mut moved = refresh.damage.iter();
        let (node, damage) = moved.next().ok_or(DamageRejection::NothingMoved)?;
        if moved.next().is_some() {
            return Err(DamageRejection::SeveralTerminalsMoved);
        }

        let rows = match damage {
            TerminalDamage::Rows(rows) => rows,
            TerminalDamage::Full => return Err(DamageRejection::FullDamage),
            TerminalDamage::None => return Err(DamageRejection::NothingMoved),
        };

        // `render_regions` falls back to painting the whole frame when the root is gone, which
        // would write far outside the rows this draw restores. Nothing else guards that.
        if !self.core.tree.is_valid(self.core.tree.root) || !self.core.tree.is_valid(*node) {
            return Err(DamageRejection::NodeMissing);
        }
        let NodeKind::Terminal(terminal) = &self.core.tree.node(*node).kind else {
            return Err(DamageRejection::NodeMissing);
        };
        if terminal.scrollback_offset != 0 {
            return Err(DamageRejection::ScrolledBack);
        }
        if terminal.border {
            return Err(DamageRejection::BorderedTerminal);
        }
        #[cfg(feature = "terminal-images")]
        if !terminal.images.is_empty() {
            return Err(DamageRejection::HasImages);
        }

        let snapshot = self
            .last_frame_snapshot
            .as_ref()
            .ok_or(DamageRejection::NoMatchingRetainedFrame)?;
        let node_rect = self.core.tree.node(*node).rect;
        if node_rect.w == 0 || node_rect.h == 0 {
            return Err(DamageRejection::NoMatchingRetainedFrame);
        }
        // The retained frame is what the patch is applied to and diffed against, so it has to
        // describe the same screen. Deciding this here rather than in the draw is what makes a
        // `TerminalDamagePlan` mean "executable" instead of "promising": the draw should never
        // discover that its plan was invalid.
        if *snapshot.area() != frame_area || frame_area.width == 0 || frame_area.height == 0 {
            return Err(DamageRejection::NoMatchingRetainedFrame);
        }

        // An undecorated terminal's content starts at its rect plus padding, so a viewport row is
        // a fixed offset from a screen row. Rows past the content, or off the frame, are dropped:
        // nothing shows them, so nothing has to repaint them.
        let content_top = i32::from(node_rect.y) + i32::from(terminal.padding.top);
        let content_rows = node_rect
            .h
            .saturating_sub(terminal.padding.top.saturating_add(terminal.padding.bottom));
        let frame_top = i32::from(frame_area.y);
        let frame_bottom = frame_top + i32::from(frame_area.height);
        let mut rows: Vec<u16> = rows
            .iter()
            .filter(|row| row.row < content_rows)
            .map(|row| content_top + i32::from(row.row))
            .filter(|row| *row >= frame_top && *row < frame_bottom)
            .map(|row| row as u16)
            .collect();
        if rows.is_empty() {
            return Err(DamageRejection::NothingVisible);
        }

        // The caret is placed by whichever row paints the focused widget: the renderers record it
        // through `CursorPlacement` as that row is drawn, exactly as in a full paint. A caret on a
        // row the terminal did not damage would therefore never be recorded, and the draw would
        // have nothing to place - leaving the host caret wherever the last cell write left it.
        // Painting that one extra row is what lets the draw treat the recorded position as the
        // whole answer, including its decision *not* to show a caret at all.
        if let Some(caret) = self.incremental_cursor_position()
            && i32::from(caret.y) >= frame_top
            && i32::from(caret.y) < frame_bottom
            && !rows.contains(&caret.y)
        {
            rows.push(caret.y);
            rows.sort_unstable();
        }

        Ok(TerminalDamagePlan { node: *node, rows })
    }

    /// [`plan_terminal_damage`](Self::plan_terminal_damage), with the reason logged and discarded.
    pub(crate) fn prepare_terminal_damage_plan(
        &self,
        refresh: &LiveTerminalRefresh,
        frame_area: ratatui::layout::Rect,
    ) -> Option<TerminalDamagePlan> {
        match self.plan_terminal_damage(refresh, frame_area) {
            Ok(plan) => Some(plan),
            Err(reason) => {
                crate::debug::internal_log!("[tui-lipan] terminal damage fallback: {reason:?}");
                None
            }
        }
    }

    /// Repaint the rows a plan names, and nothing else.
    ///
    /// Each row is painted by the ordinary tree walk clipped to that row, over Ratatui's current
    /// buffer used as scratch, seeded with the retained frame's copy of the row. Only that row is
    /// read, written, compared or sent, so the cost tracks row width and one tree traversal rather
    /// than the size of the window.
    ///
    /// The retained frame advances one row at a time, at the moment the host accepts that row.
    /// That is the transaction boundary and it is not the end of this function: a row the backend
    /// rejected must not be recorded as shown, and a row it accepted must be recorded even if the
    /// cursor sync or the flush afterwards fails, because the host already has it.
    pub(super) fn draw_terminal_damage(
        &mut self,
        terminal: &mut crate::backend::ratatui_backend::Terminal,
        plan: &TerminalDamagePlan,
    ) -> crate::Result<()> {
        // Late-bound paints name their transitions by key and resolve them here, exactly as the
        // ordinary draw does. Dropped at the end of this draw.
        let _animations = crate::animation::registry::set_render_registry(std::rc::Rc::clone(
            &self.core.ctx.env().animations,
        ));

        let frame_area = terminal.get_frame().area();
        let cursor_position = StdCell::new(None);
        let mut snapshot = self
            .last_frame_snapshot
            .take()
            .expect("the plan proved a retained frame of this geometry");

        let _ = crossterm::execute!(
            terminal.backend_mut(),
            crossterm::style::Print("\x1b[?2026h")
        );
        let mut painted: Vec<u16> = Vec::with_capacity(plan.rows.len());
        let result: crate::Result<()> = self.with_render_context(&cursor_position, |ctx| {
            for &row in &plan.rows {
                patch_row(terminal, ctx, &mut snapshot, frame_area, row)?;
                painted.push(row);
            }
            // Where the paint above asked for the caret, which is the whole answer because the
            // plan guaranteed the caret's own row was painted. Inside the synchronized update and
            // inside the render context, for the same reason `Terminal::draw` does it before
            // returning: the host must not be left showing a caret from a half-finished frame.
            place_caret(terminal, cursor_position.get())?;
            Ok(())
        });
        let _ = crossterm::execute!(
            terminal.backend_mut(),
            crossterm::style::Print("\x1b[?2026l")
        );

        self.last_frame_snapshot = Some(snapshot);
        self.host_patched_rows.extend(painted);
        result?;

        self.terminal.update_cursor(
            terminal.backend_mut(),
            &self.core.tree,
            self.focus.focused,
            &self.widgets.text_area_vim_state,
        )?;
        terminal.backend_mut().flush()?;
        // Deliberately no `swap_buffers`. Ratatui's current buffer was borrowed as scratch and put
        // back as it was found; its previous buffer still describes the last full draw, and
        // `host_patched_rows` is what reconciles that with the rows written here.
        Ok(())
    }
}

/// Repaint one physical row and send only what changed on it to the host.
///
/// Ratatui's current buffer is the scratch surface, chosen so the paint sees the real viewport
/// geometry rather than a buffer that merely looks like it. It is saved, blanked, painted, read
/// back and restored - and the restore runs before anything fallible does, which is what makes it
/// unconditional: there is no error path between taking the row and putting it back.
///
/// The retained frame's copy of the row is the diff baseline, and only that. It is deliberately
/// not the paint's starting point: see `blank_row` below.
///
/// Generic over the backend only so a test can hand it a `TestBackend` and read back what the host
/// was actually sent. Nothing here is backend-specific.
pub(super) fn patch_row<B: Backend>(
    terminal: &mut ratatui::Terminal<B>,
    ctx: &RenderContext<'_>,
    snapshot: &mut Buffer,
    frame_area: RatatuiRect,
    row: u16,
) -> std::result::Result<(), B::Error> {
    let row_area = RatatuiRect::new(frame_area.x, row, frame_area.width, 1);
    let previous = read_row(snapshot, row_area);

    let next = {
        let mut frame = terminal.get_frame();
        let scratch = read_row(frame.buffer_mut(), row_area);
        // A full draw paints into a buffer Ratatui has just reset, so every cell no widget writes
        // ends up blank - including the trailing half of a wide glyph, which the renderer skips
        // rather than clears. Seeding the retained frame's cells here instead would leave that
        // half showing whatever used to be under it. Blank is what the paint expects to find.
        blank_row(frame.buffer_mut(), row_area);
        // Then the opt-in root background, which `render` paints first and `render_regions` does
        // not, because a region repaint has no root of its own to fill.
        if let Some(background) =
            crate::backend::ratatui_backend::common::current_render_screen_background()
        {
            frame.render_widget(
                ratatui::widgets::Block::default().style(background),
                row_area,
            );
        }
        render_regions(
            &mut frame,
            ctx,
            &[crate::style::Rect {
                x: row_area.x as i16,
                y: row_area.y as i16,
                w: row_area.width,
                h: 1,
            }],
        );
        let next = read_row(frame.buffer_mut(), row_area);
        write_row(frame.buffer_mut(), &scratch);
        next
    };

    // Ratatui's own diff over a one-row buffer, so multi-width glyphs and per-cell diff options
    // behave exactly as they do in a full draw. The row buffers carry their absolute position, so
    // the updates are already in screen coordinates.
    let updates = previous.diff(&next);
    if !updates.is_empty() {
        terminal.backend_mut().draw(updates.into_iter())?;
    }
    // The host has this row. Recording it cannot wait for the rest of the frame to succeed.
    write_row(snapshot, &next);
    Ok(())
}

/// Put the host caret where the paint asked for it, or hide it when no paint asked for one.
///
/// The rule [`ratatui::Terminal::draw`] applies once a full draw finishes, applied by hand because
/// this draw never calls it. Without it the caret is not merely stale: every backend write leaves
/// it one column past the run it just sent, so it wanders to wherever the last patched row
/// happened to end.
///
/// Generic over the backend for the same reason [`patch_row`] is - so a test can watch what the
/// host was told.
pub(super) fn place_caret<B: Backend>(
    terminal: &mut ratatui::Terminal<B>,
    caret: Option<Position>,
) -> std::result::Result<(), B::Error> {
    match caret {
        Some(position) => {
            terminal.show_cursor()?;
            terminal.set_cursor_position(position)?;
        }
        None => terminal.hide_cursor()?,
    }
    Ok(())
}

/// Reset one row of a buffer, leaving it as a fresh draw would find it.
fn blank_row(target: &mut Buffer, row_area: RatatuiRect) {
    for x in row_area.left()..row_area.right() {
        target[(x, row_area.y)].reset();
    }
}

/// Copy one row out of a full-frame buffer into a buffer of just that row.
fn read_row(source: &Buffer, row_area: RatatuiRect) -> Buffer {
    let mut row = Buffer::empty(row_area);
    for x in row_area.left()..row_area.right() {
        row[(x, row_area.y)] = source[(x, row_area.y)].clone();
    }
    row
}

/// Copy a one-row buffer back over the same row of a full-frame buffer.
fn write_row(target: &mut Buffer, row: &Buffer) {
    let area = row.area;
    for x in area.left()..area.right() {
        target[(x, area.y)] = row[(x, area.y)].clone();
    }
}

/// Put Ratatui's previous buffer back in agreement with the host over rows a patch wrote directly.
///
/// Called once, on the next ordinary draw, with the rows as the host holds them, the frame that
/// draw just produced, and the caret that draw placed. Every cell that differs is sent again;
/// cells Ratatui's own diff already sent are simply re-sent, which is cheaper than tracking which
/// ones those were.
///
/// The caret is a parameter because this runs *after* the draw placed it, and sending cells moves
/// it: a backend prints each cell where it sits and ends one column past the last symbol. Anything
/// re-sent here would therefore leave the caret at the end of the last row this touched. Restoring
/// it belongs to the function that disturbs it, not to the caller that has already finished.
pub(super) fn resync_host_patched_rows<B: Backend>(
    terminal: &mut ratatui::Terminal<B>,
    host_rows: &[Buffer],
    frame: &Buffer,
    caret: Option<Position>,
) -> std::result::Result<(), B::Error> {
    let mut sent = false;
    for previous in host_rows {
        if !frame.area.contains(ratatui::layout::Position::new(
            previous.area.x,
            previous.area.y,
        )) || previous.area.right() > frame.area.right()
        {
            continue;
        }
        let next = read_row(frame, previous.area);
        let updates = previous.diff(&next);
        if !updates.is_empty() {
            terminal.backend_mut().draw(updates.into_iter())?;
            sent = true;
        }
    }
    if sent {
        place_caret(terminal, caret)?;
    }
    Ok(())
}

/// Copy the rows a patch wrote to the host out of the retained frame, before it is replaced.
pub(super) fn take_host_patched_rows(
    rows: &mut Vec<u16>,
    snapshot: Option<&Buffer>,
) -> Vec<Buffer> {
    let drained = std::mem::take(rows);
    let Some(snapshot) = snapshot else {
        return Vec::new();
    };
    drained
        .into_iter()
        .filter(|row| *row >= snapshot.area.y && *row < snapshot.area.bottom())
        .map(|row| {
            read_row(
                snapshot,
                RatatuiRect::new(snapshot.area.x, row, snapshot.area.width, 1),
            )
        })
        .collect()
}
