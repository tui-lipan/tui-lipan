use crossterm::{cursor::MoveTo, execute};
use ratatui::backend::Backend;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use std::collections::HashMap;
use std::ops::Range;

use crate::Result;
use crate::backend::ratatui_backend::{RenderContext, render_regions};
use crate::core::component::Component;
use crate::core::node::NodeId;
use crate::style::Rect;

use super::super::scroll_optimize::{
    IncrementalScrollPlan, clear_buffer_rows, collect_scroll_repaint_regions, shift_buffer_rows,
    subtree_has_hoverables,
};
use super::super::{ActiveDrag, ScrollFrameSnapshot};
use super::{AppRunner, DrawMode};

impl<C: Component> AppRunner<C> {
    pub(super) fn active_selection_drag_requires_full_repaint(&self) -> bool {
        matches!(
            self.drag.active,
            ActiveDrag::TextArea(_)
                | ActiveDrag::DocumentView(_)
                | ActiveDrag::Input(_)
                | ActiveDrag::HexArea(_)
        )
    }

    pub(super) fn prepare_incremental_scroll_plan(
        &self,
        draw_mode: DrawMode,
        frame_area: ratatui::layout::Rect,
        current_scroll_frames: &[ScrollFrameSnapshot],
    ) -> Option<IncrementalScrollPlan> {
        if draw_mode != DrawMode::LayoutOnly
            || self.surface.is_inline()
            || frame_area.width == 0
            || frame_area.height == 0
            || !self.core.tree.overlay_roots().is_empty()
            || self.active_selection_drag_requires_full_repaint()
            || {
                #[cfg(feature = "devtools")]
                {
                    self.devtools_state.borrow().visible
                }
                #[cfg(not(feature = "devtools"))]
                {
                    false
                }
            }
        {
            return None;
        }

        let previous_by_id: HashMap<_, _> = self
            .last_scroll_frames
            .iter()
            .map(|frame| (frame.node_id, frame))
            .collect();

        let mut candidates = current_scroll_frames.iter().filter_map(|current| {
            let previous = previous_by_id.get(&current.node_id)?;
            if current.scroll_offset == previous.scroll_offset {
                return None;
            }
            Some((current, (*previous).clone()))
        });

        let (current, previous) = candidates.next()?;
        if candidates.next().is_some() {
            return None;
        }

        if self.scroll_hover_requires_full_repaint(current.node_id, &current.scroll_rows) {
            return None;
        }

        let snapshot = self.last_frame_snapshot.as_ref()?;
        if snapshot.area != frame_area {
            return None;
        }

        if current.show_scroll_indicators
            || previous.show_scroll_indicators
            || current.scroll_rows != previous.scroll_rows
            || current.content_hash.is_none()
            || current.content_hash != previous.content_hash
            || current.content_height != previous.content_height
            || current.viewport_height != previous.viewport_height
            || current.scroll_rows.start >= current.scroll_rows.end
        {
            return None;
        }

        let delta_rows = current.scroll_offset as i32 - previous.scroll_offset as i32;
        if delta_rows == 0 {
            return None;
        }

        let delta_rows = delta_rows as i16;
        let visible_rows = current
            .scroll_rows
            .end
            .saturating_sub(current.scroll_rows.start);
        if delta_rows.unsigned_abs() == 0 || delta_rows.unsigned_abs() >= visible_rows {
            return None;
        }

        let exposed_rows = if delta_rows > 0 {
            current.scroll_rows.end - delta_rows as u16..current.scroll_rows.end
        } else {
            current.scroll_rows.start..current.scroll_rows.start + delta_rows.unsigned_abs()
        };

        let mut repaint_regions = vec![Rect {
            x: frame_area.x as i16,
            y: frame_area.y.saturating_add(exposed_rows.start) as i16,
            w: frame_area.width,
            h: exposed_rows.end.saturating_sub(exposed_rows.start),
        }];

        if let Some(scrollbar_rect) = current.scrollbar_rect.or(previous.scrollbar_rect) {
            repaint_regions.push(Rect {
                x: scrollbar_rect.x.saturating_add(frame_area.x as i16),
                y: scrollbar_rect.y.saturating_add(frame_area.y as i16),
                w: scrollbar_rect.w,
                h: scrollbar_rect.h,
            });
        }

        let scroll_band = Rect {
            x: 0,
            y: current.scroll_rows.start as i16,
            w: frame_area.width,
            h: visible_rows,
        };
        repaint_regions.extend(collect_scroll_repaint_regions(
            &self.core.tree,
            current.node_id,
            scroll_band,
            frame_area,
        ));

        Some(IncrementalScrollPlan {
            scroll_rows: frame_area.y.saturating_add(current.scroll_rows.start)
                ..frame_area.y.saturating_add(current.scroll_rows.end),
            delta_rows,
            repaint_regions,
        })
    }

    fn scroll_hover_requires_full_repaint(
        &self,
        scroll_id: NodeId,
        scroll_rows: &Range<u16>,
    ) -> bool {
        if !self.core.tree.has_hoverables() || !subtree_has_hoverables(&self.core.tree, scroll_id) {
            return false;
        }

        let Some((mx, my)) = self.mouse.last_mouse else {
            return false;
        };

        if my < scroll_rows.start || my >= scroll_rows.end {
            return false;
        }

        self.core
            .tree
            .node(scroll_id)
            .rect
            .contains(mx as i16, my as i16)
    }

    pub(super) fn draw_incremental_scroll(
        terminal: &mut crate::backend::ratatui_backend::Terminal,
        ctx: &RenderContext<'_>,
        plan: &IncrementalScrollPlan,
        last_snapshot: &mut Buffer,
        diff_snapshot: &mut Buffer,
        _cursor_requested: bool,
    ) -> Result<()> {
        diff_snapshot.clone_from(last_snapshot);

        let exposed_rows = if plan.delta_rows > 0 {
            plan.scroll_rows.end - plan.delta_rows as u16..plan.scroll_rows.end
        } else {
            plan.scroll_rows.start..plan.scroll_rows.start + plan.delta_rows.unsigned_abs()
        };
        {
            let mut frame = terminal.get_frame();
            {
                let rendered = frame.buffer_mut();
                rendered.clone_from(last_snapshot);
                shift_buffer_rows(rendered, &plan.scroll_rows, plan.delta_rows);
                clear_buffer_rows(rendered, &exposed_rows);
            }
            render_regions(&mut frame, ctx, &plan.repaint_regions);
            last_snapshot.clone_from(frame.buffer_mut());
        }

        shift_buffer_rows(diff_snapshot, &plan.scroll_rows, plan.delta_rows);
        clear_buffer_rows(diff_snapshot, &exposed_rows);

        let updates = diff_snapshot.diff(last_snapshot);

        if plan.delta_rows > 0 {
            terminal
                .backend_mut()
                .scroll_region_up(plan.scroll_rows.clone(), plan.delta_rows as u16)?;
        } else {
            terminal
                .backend_mut()
                .scroll_region_down(plan.scroll_rows.clone(), plan.delta_rows.unsigned_abs())?;
        }
        terminal.backend_mut().draw(updates.into_iter())?;

        if let Some(cursor_pos) = ctx.cursor_position.get() {
            terminal.show_cursor()?;
            terminal.set_cursor_position(Position {
                x: cursor_pos.x,
                y: cursor_pos.y,
            })?;
        } else {
            terminal.hide_cursor()?;
            execute!(terminal.backend_mut(), MoveTo(0, 0))?;
        }

        terminal.swap_buffers();
        terminal.backend_mut().flush()?;
        Ok(())
    }
}
