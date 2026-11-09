//! Repainting retained frame rows from live content and runtime-owned widget damage.
//!
//! Planning is separated from drawing so eligibility can be asserted without rendering anything:
//! a test can hand this a tree and a refresh result and check *why* a case was rejected. `None`
//! is always the safe answer, and means the caller does an ordinary paint over the tree it has
//! already refreshed.
//!
//! The draw is deliberately not a second renderer. It runs the ordinary tree walk through the
//! production render context, clipped to a band of rows, so a patched row is by construction the
//! same cells a full paint would have produced there.

use ratatui::backend::Backend;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect as RatatuiRect};
use std::cell::Cell as StdCell;

use crate::app::AppRunner;
use crate::backend::ratatui_backend::{RenderContext, render_regions};
use crate::core::component::Component;
use crate::core::node::NodeId;
#[cfg(feature = "terminal")]
use crate::core::node::{LiveTerminalRefresh, NodeKind};
#[cfg(feature = "terminal")]
use crate::widgets::internal::TerminalDamage;

/// Why a frame could not be repainted from damage. Recorded rather than returned as a bare `None`
/// so the reason is testable and shows up in debug logs instead of being inferred.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DamageRejection {
    /// No live content or runtime-owned widget reported anything.
    NothingMoved,
    /// More than half the frame's rows changed, so patching them costs more than an ordinary
    /// single tree walk.
    DenseDamage,
    /// The terminal reported `Full` - a resize, a screen swap, decorations.
    #[cfg(feature = "terminal")]
    FullDamage,
    /// A snapshot changed but its damage was already consumed by another screen observer.
    #[cfg(feature = "terminal")]
    MissingDamage,
    /// A damaged node is gone or a terminal damage entry no longer names a terminal.
    NodeMissing,
    /// There is no retained frame to patch, or it does not match the current geometry.
    NoMatchingRetainedFrame,
    /// An overlay, devtools panel, drag preview or inline surface composites over the tree, so a
    /// terminal row is not the only thing that could occupy those cells.
    CompositeSurface,
    /// The terminal is showing scrollback rather than the live viewport.
    #[cfg(feature = "terminal")]
    ScrolledBack,
    /// The terminal carries images, which are not painted from the cell grid.
    #[cfg_attr(not(feature = "terminal-images"), allow(dead_code))]
    HasImages,
    /// The terminal draws a border, so which physical row a viewport row lands on depends on
    /// which borders the clip leaves visible. Padding is a fixed offset and stays eligible.
    #[cfg(feature = "terminal")]
    BorderedTerminal,
    /// Every damaged row falls outside the frame, so there is nothing to repaint.
    NothingVisible,
}

/// Live content and runtime-owned widgets' damage, resolved to physical frame rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RowDamagePlan {
    /// Physical frame rows this repaint has to paint, ascending, each inside the frame: every row
    /// the terminal moved, plus the row the focused widget's caret sits on.
    ///
    /// Resolved here rather than in the draw: the mapping from a viewport row to a screen row is
    /// geometry, and geometry is what the planner is for. The draw only paints rows.
    pub rows: Vec<u16>,
}

/// The most undamaged rows a band paints to join two damaged rows into one tree walk.
///
/// Each band costs one walk of the whole tree, and every widget on its rows pays its own setup
/// again, which for a terminal includes resolving the image layers above it. An extra row costs
/// one row of cells to blank, paint and compare, and sends nothing when it did not change. Rows
/// that change together, such as the spinners of a list, usually sit a row or two apart.
const BAND_GAP: u16 = 4;

impl RowDamagePlan {
    /// The rows to paint in a frame `frame_height` rows tall, as ascending half-open bands. Each
    /// damaged row is in exactly one band.
    ///
    /// Two damaged rows share a band when at most [`BAND_GAP`] undamaged rows lie between them.
    /// Joining is transitive, so damage spread evenly down the frame could otherwise grow into one
    /// band nearly as tall as the frame. The bands are held to the same half-frame budget the
    /// planner applies to damaged rows: past it, every damaged row is its own band again.
    pub(crate) fn bands(&self, frame_height: u16) -> Vec<std::ops::Range<u16>> {
        let mut bands: Vec<std::ops::Range<u16>> = Vec::new();
        for &row in &self.rows {
            match bands.last_mut() {
                Some(band) if row >= band.end && row - band.end <= BAND_GAP => band.end = row + 1,
                _ => bands.push(row..row + 1),
            }
        }
        let painted: usize = bands.iter().map(ExactSizeIterator::len).sum();
        let budget = (usize::from(frame_height) / 2).max(self.rows.len());
        if painted > budget {
            return self.rows.iter().map(|&row| row..row + 1).collect();
        }
        bands
    }
}

impl<C> AppRunner<C>
where
    C: Component + 'static,
{
    fn validate_damage_surface(
        &self,
        frame_area: ratatui::layout::Rect,
    ) -> Result<(), DamageRejection> {
        self.validate_damage_compositing()?;
        self.validate_retained_damage_frame(frame_area)?;
        #[cfg(feature = "terminal-images")]
        self.validate_damage_images(frame_area)?;
        Ok(())
    }

    fn validate_damage_compositing(&self) -> Result<(), DamageRejection> {
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
        Ok(())
    }

    fn validate_retained_damage_frame(
        &self,
        frame_area: ratatui::layout::Rect,
    ) -> Result<(), DamageRejection> {
        if !self.core.tree.is_valid(self.core.tree.root) {
            return Err(DamageRejection::NodeMissing);
        }
        let snapshot = self
            .last_frame_snapshot
            .as_ref()
            .ok_or(DamageRejection::NoMatchingRetainedFrame)?;
        if *snapshot.area() != frame_area || frame_area.width == 0 || frame_area.height == 0 {
            return Err(DamageRejection::NoMatchingRetainedFrame);
        }
        Ok(())
    }

    #[cfg(feature = "terminal-images")]
    fn validate_damage_images(
        &self,
        frame_area: ratatui::layout::Rect,
    ) -> Result<(), DamageRejection> {
        // Static native images can keep their complete scene while cells are painted at a row
        // clip. Composed images still require a whole paint to preserve their protocol placements.
        let cached_plans_valid =
            self.graphics_plan_epoch == Some(self.graphics_presentation_epochs(frame_area));
        if self.core.tree.iter().any(|node| {
            matches!(&node.kind, NodeKind::Terminal(terminal)
            if !terminal.images.is_empty() && (!self.native_terminal_plans.contains(&node.id)
                || !cached_plans_valid))
        }) {
            return Err(DamageRejection::HasImages);
        }
        Ok(())
    }

    /// Combine terminal viewport damage and fixed-allocation node damage before any drawing.
    /// Unknown changes or an unsafe surface always fall back to the ordinary full paint.
    #[cfg(feature = "terminal")]
    pub(crate) fn plan_live_damage(
        &self,
        refresh: &LiveTerminalRefresh,
        nodes: &[NodeId],
        frame_area: ratatui::layout::Rect,
    ) -> Result<RowDamagePlan, DamageRejection> {
        self.validate_damage_surface(frame_area)?;
        if refresh.damage.is_empty() && refresh.text_damage.is_empty() && nodes.is_empty() {
            return Err(DamageRejection::NothingMoved);
        }
        let mut rows = Vec::new();
        for (node, damage) in &refresh.damage {
            self.add_terminal_damage(&mut rows, *node, damage, frame_area)?;
        }
        self.add_node_damage(&mut rows, &refresh.text_damage, frame_area)?;
        self.add_node_damage(&mut rows, nodes, frame_area)?;
        self.finish_damage_plan(rows, frame_area)
    }

    #[cfg(feature = "terminal")]
    fn add_terminal_damage(
        &self,
        rows: &mut Vec<u16>,
        node: NodeId,
        damage: &TerminalDamage,
        frame_area: ratatui::layout::Rect,
    ) -> Result<(), DamageRejection> {
        let damaged = match damage {
            TerminalDamage::Rows(rows) => rows,
            TerminalDamage::Full => return Err(DamageRejection::FullDamage),
            // Shared screen observers can consume damage before another mounted widget sees
            // the newer snapshot. Without its own rows that widget needs a complete paint.
            TerminalDamage::None => return Err(DamageRejection::MissingDamage),
        };
        let (content_top, content_rows) = self.terminal_damage_geometry(node)?;
        let frame_top = i32::from(frame_area.y);
        let frame_bottom = frame_top + i32::from(frame_area.height);
        rows.extend(
            damaged
                .iter()
                .filter(|row| row.row < content_rows)
                .map(|row| content_top + i32::from(row.row))
                .filter(|row| *row >= frame_top && *row < frame_bottom)
                .map(|row| row as u16),
        );
        Ok(())
    }

    #[cfg(feature = "terminal")]
    fn terminal_damage_geometry(&self, node: NodeId) -> Result<(i32, u16), DamageRejection> {
        if !self.core.tree.is_valid(node) {
            return Err(DamageRejection::NodeMissing);
        }
        let node = self.core.tree.node(node);
        let NodeKind::Terminal(terminal) = &node.kind else {
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
        if node.rect.w == 0 || node.rect.h == 0 {
            return Err(DamageRejection::NoMatchingRetainedFrame);
        }
        Ok((
            i32::from(node.rect.y) + i32::from(terminal.padding.top),
            node.rect
                .h
                .saturating_sub(terminal.padding.top.saturating_add(terminal.padding.bottom)),
        ))
    }

    /// Fixed-allocation nodes without live terminals use the same planner and retained-row draw.
    #[cfg(not(feature = "terminal"))]
    pub(crate) fn plan_node_damage(
        &self,
        nodes: &[NodeId],
        frame_area: ratatui::layout::Rect,
    ) -> Result<RowDamagePlan, DamageRejection> {
        self.validate_damage_surface(frame_area)?;
        if nodes.is_empty() {
            return Err(DamageRejection::NothingMoved);
        }
        let mut rows = Vec::new();
        self.add_node_damage(&mut rows, nodes, frame_area)?;
        self.finish_damage_plan(rows, frame_area)
    }

    fn add_node_damage(
        &self,
        rows: &mut Vec<u16>,
        nodes: &[NodeId],
        frame_area: ratatui::layout::Rect,
    ) -> Result<(), DamageRejection> {
        let frame_top = i32::from(frame_area.y);
        let frame_bottom = frame_top + i32::from(frame_area.height);
        for id in nodes {
            if !self.core.tree.is_valid(*id) {
                return Err(DamageRejection::NodeMissing);
            }
            let rect = self.core.tree.node(*id).rect;
            rows.extend(
                (i32::from(rect.y)..i32::from(rect.y) + i32::from(rect.h))
                    .filter(|row| *row >= frame_top && *row < frame_bottom)
                    .map(|row| row as u16),
            );
        }
        Ok(())
    }

    fn finish_damage_plan(
        &self,
        mut rows: Vec<u16>,
        frame_area: ratatui::layout::Rect,
    ) -> Result<RowDamagePlan, DamageRejection> {
        let frame_top = i32::from(frame_area.y);
        let frame_bottom = frame_top + i32::from(frame_area.height);
        rows.sort_unstable();
        rows.dedup();
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

        if rows.len() > usize::from(frame_area.height) / 2 {
            return Err(DamageRejection::DenseDamage);
        }
        Ok(RowDamagePlan { rows })
    }

    /// [`plan_live_damage`](Self::plan_live_damage) for terminal-only frames, logging any fallback.
    #[cfg(feature = "terminal")]
    pub(crate) fn prepare_terminal_damage_plan(
        &self,
        refresh: &LiveTerminalRefresh,
        frame_area: ratatui::layout::Rect,
    ) -> Option<RowDamagePlan> {
        match self.plan_live_damage(refresh, &[], frame_area) {
            Ok(plan) => Some(plan),
            Err(reason) => {
                crate::debug::internal_log!("[tui-lipan] terminal damage fallback: {reason:?}");
                None
            }
        }
    }

    /// Repaint the plan's bands: the rows it names, plus the undamaged rows a band joins them
    /// across (see [`RowDamagePlan::bands`]).
    ///
    /// Each band is painted by one ordinary tree walk clipped to that band, over Ratatui's current
    /// buffer used as scratch. Only the band is read, written, compared or sent, so the cost tracks
    /// the band's cells and one tree traversal per band rather than the size of the window. A row
    /// painted only because a band crosses it comes out as it already was and sends nothing.
    ///
    /// The retained frame advances one band at a time, at the moment the host accepts that band.
    /// That is the transaction boundary and it is not the end of this function: a band the backend
    /// rejected must not be recorded as shown, and a band it accepted must be recorded even if the
    /// cursor sync or the flush afterwards fails, because the host already has it.
    pub(super) fn draw_row_damage(
        &mut self,
        terminal: &mut crate::backend::ratatui_backend::Terminal,
        plan: &RowDamagePlan,
    ) -> crate::Result<()> {
        // Late-bound paints name their transitions by key and resolve them here, exactly as the
        // ordinary draw does. Dropped at the end of this draw.
        let _animations = crate::animation::registry::set_render_registry(
            std::rc::Rc::clone(&self.core.ctx.env().animations),
            self.core.ctx.env().clock.elapsed(),
            crate::animation::registry::PaintExtent::Partial,
        );

        #[cfg(feature = "terminal-images")]
        let _native_plans = terminal.backend().native_plan_scope();
        #[cfg(feature = "terminal")]
        let presentations = crate::widgets::presentation::PresentationScope::begin();
        let output_frame = terminal.backend().begin_output_frame()?;
        let output_epoch = terminal.backend().output_epoch();
        let frame_area = terminal.get_frame().area();
        let cursor_position = StdCell::new(None);
        let mut snapshot = self
            .last_frame_snapshot
            .take()
            .expect("the plan proved a retained frame of this geometry");

        crossterm::execute!(
            terminal.backend_mut(),
            crossterm::style::Print("\x1b[?2026h")
        )?;
        let mut painted: Vec<u16> = Vec::with_capacity(plan.rows.len());
        let result: crate::Result<()> = self.with_render_context(&cursor_position, |ctx| {
            for band in plan.bands(frame_area.height) {
                patch_rows(terminal, ctx, &mut snapshot, frame_area, band.clone())?;
                painted.extend(band);
            }
            Ok(())
        });
        // Where the paint above asked for the caret, which is the whole answer because the plan
        // guaranteed the caret's own row was painted. The cursor trail sees it first: a jump a
        // solid trail will draw keeps the caret hidden in this frame rather than showing it at its
        // destination until the next full draw paints the trail. Inside the synchronized update,
        // for the same reason `Terminal::draw` places it before returning: the host must not be
        // left showing a caret from a half-finished frame.
        let placed = if result.is_ok() {
            self.observe_cursor_trail(cursor_position.get());
            let caret = cursor_position
                .get()
                .filter(|_| !self.cursor_trail.hides_caret());
            place_caret(terminal, caret)
        } else {
            Ok(())
        };
        crossterm::execute!(
            terminal.backend_mut(),
            crossterm::style::Print("\x1b[?2026l")
        )?;

        self.last_frame_snapshot = Some(snapshot);
        // A continuously animated spinner may run for hours without a full paint. Retain each
        // physical row only once, rather than accumulating one entry per animation frame.
        for row in painted {
            if !self.host_patched_rows.contains(&row) {
                self.host_patched_rows.push(row);
            }
        }
        result?;
        placed?;
        self.frame_rendered = true;

        self.terminal.update_cursor(
            terminal.backend_mut(),
            &self.core.tree,
            self.focus.focused,
            &self.widgets.text_area_vim_state,
        )?;
        terminal.backend_mut().finish_output_frame(output_frame)?;
        self.host_frame_presented = terminal.backend().output_epoch() == output_epoch;
        #[cfg(feature = "terminal")]
        if self.host_frame_presented {
            self.core
                .ctx
                .env()
                .terminal_presentation_observers
                .deliver(presentations.take());
        }
        // Deliberately no `swap_buffers`. Ratatui's current buffer was borrowed as scratch and put
        // back as it was found; its previous buffer still describes the last full draw, and
        // `host_patched_rows` is what reconciles that with the rows written here.
        Ok(())
    }
}

/// Repaint a band of physical rows in one tree walk and send only what changed to the host.
///
/// Ratatui's current buffer is the scratch surface, chosen so the paint sees the real viewport
/// geometry rather than a buffer that merely looks like it. It is saved, blanked, painted, read
/// back and restored - and the restore runs before anything fallible does, which is what makes it
/// unconditional: there is no error path between taking the band and putting it back.
///
/// The retained frame's copy of the band is the diff baseline, and only that. It is deliberately
/// not the paint's starting point: see `blank_area` below.
///
/// Generic over the backend only so a test can hand it a `TestBackend` and read back what the host
/// was actually sent. Nothing here is backend-specific.
pub(super) fn patch_rows<B: Backend>(
    terminal: &mut ratatui::Terminal<B>,
    ctx: &RenderContext<'_>,
    snapshot: &mut Buffer,
    frame_area: RatatuiRect,
    rows: std::ops::Range<u16>,
) -> std::result::Result<(), B::Error> {
    let band = RatatuiRect::new(
        frame_area.x,
        rows.start,
        frame_area.width,
        rows.end.saturating_sub(rows.start),
    );
    if band.is_empty() {
        return Ok(());
    }
    let previous = read_area(snapshot, band);

    let next = {
        let mut frame = terminal.get_frame();
        let scratch = read_area(frame.buffer_mut(), band);
        // A full draw paints into a buffer Ratatui has just reset, so every cell no widget writes
        // ends up blank - including the trailing half of a wide glyph, which the renderer skips
        // rather than clears. Seeding the retained frame's cells here instead would leave that
        // half showing whatever used to be under it. Blank is what the paint expects to find.
        blank_area(frame.buffer_mut(), band);
        // Then the opt-in root background, which `render` paints first and `render_regions` does
        // not, because a region repaint has no root of its own to fill.
        if let Some(background) =
            crate::backend::ratatui_backend::common::current_render_screen_background()
        {
            frame.render_widget(ratatui::widgets::Block::default().style(background), band);
        }
        render_regions(
            &mut frame,
            ctx,
            &[crate::style::Rect {
                x: band.x as i16,
                y: band.y as i16,
                w: band.width,
                h: band.height,
            }],
        );
        let next = read_area(frame.buffer_mut(), band);
        write_area(frame.buffer_mut(), &scratch);
        next
    };

    // Ratatui's own diff over a buffer of just the band, so multi-width glyphs and per-cell diff
    // options behave exactly as they do in a full draw. The band buffers carry their absolute
    // position, so the updates are already in screen coordinates.
    let updates = previous.diff(&next);
    if !updates.is_empty() {
        terminal.backend_mut().draw(updates.into_iter())?;
    }
    // The host has these rows. Recording them cannot wait for the rest of the frame to succeed.
    write_area(snapshot, &next);
    Ok(())
}

/// Put the host caret where the paint asked for it, or hide it when no paint asked for one.
///
/// The rule [`ratatui::Terminal::draw`] applies once a full draw finishes, applied by hand because
/// this draw never calls it. Without it the caret is not merely stale: every backend write leaves
/// it one column past the run it just sent, so it wanders to wherever the last patched row
/// happened to end.
///
/// Generic over the backend for the same reason [`patch_rows`] is - so a test can watch what the
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

/// Reset an area of a buffer, leaving it as a fresh draw would find it.
fn blank_area(target: &mut Buffer, area: RatatuiRect) {
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            target[(x, y)].reset();
        }
    }
}

/// Copy an area out of a full-frame buffer into a buffer of just that area.
fn read_area(source: &Buffer, area: RatatuiRect) -> Buffer {
    let mut copy = Buffer::empty(area);
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            copy[(x, y)] = source[(x, y)].clone();
        }
    }
    copy
}

/// Copy a buffer of one area back over the same area of a full-frame buffer.
fn write_area(target: &mut Buffer, copy: &Buffer) {
    let area = copy.area;
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            target[(x, y)] = copy[(x, y)].clone();
        }
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
        let next = read_area(frame, previous.area);
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
            read_area(
                snapshot,
                RatatuiRect::new(snapshot.area.x, row, snapshot.area.width, 1),
            )
        })
        .collect()
}
