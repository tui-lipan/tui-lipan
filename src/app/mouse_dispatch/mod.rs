use crate::app::input::drag;
use crate::app::input::mouse;
use crate::app::input::text_area_vim::{
    sync_visual_mode_for_external_selection, text_area_vim_search_feedback_for_text,
};
use crate::app::interaction_state::ActiveDrag;
#[cfg(not(target_arch = "wasm32"))]
use crate::app::runner::AppRunner;
use crate::core::component::Component;
use crate::core::event::{MouseButton, MouseEvent, MouseKind};
use crate::core::node::NodeKind;
use crate::test_backend::TestBackend;

mod click;
mod dispatch_drag;
mod scroll;
mod test_support;

pub(crate) use click::*;
pub(crate) use dispatch_drag::*;
pub(crate) use scroll::*;
pub(crate) use test_support::*;

pub(crate) struct TextareaVimExternalSelectionParams<'a> {
    pub id: crate::core::node::NodeId,
    pub vim_motions: bool,
    pub read_only: bool,
    pub has_on_change: bool,
    pub on_vim_mode_change: Option<&'a crate::callback::Callback<crate::widgets::TextAreaVimMode>>,
    pub cursor: usize,
    pub anchor: Option<usize>,
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn dispatch_mouse_runner<C: Component>(
    runner: &mut AppRunner<C>,
    mouse: MouseEvent,
) -> bool {
    dispatch_mouse_shared(runner, mouse)
}

pub(crate) fn dispatch_mouse_test_backend<C: Component>(
    backend: &mut TestBackend<C>,
    mouse: MouseEvent,
) -> bool {
    dispatch_mouse_shared(backend, mouse)
}

pub(super) fn sync_textarea_vim_external_selection<C: Component>(
    backend: &mut TestBackend<C>,
    params: TextareaVimExternalSelectionParams<'_>,
) {
    let TextareaVimExternalSelectionParams {
        id,
        vim_motions,
        read_only,
        has_on_change,
        on_vim_mode_change,
        cursor,
        anchor,
    } = params;
    if !vim_motions || read_only || !has_on_change {
        backend.text_area_vim_state.remove(&id);
        if backend.core.tree.is_valid(id)
            && let NodeKind::TextArea(node) = &mut backend.core.tree.node_mut(id).kind
        {
            node.vim_mode = crate::widgets::TextAreaVimMode::Normal;
            node.vim_visual_line_caret = None;
            node.vim_search_feedback = None;
        }
        return;
    }

    let (mode, visual_line_caret) = {
        let state = backend.text_area_vim_state.entry(id).or_default();
        if let Some(mode) = sync_visual_mode_for_external_selection(state, cursor, anchor)
            && let Some(cb) = on_vim_mode_change
        {
            cb.emit(mode);
        }
        (state.mode, state.visual_line_caret)
    };
    let search_feedback = if backend.core.tree.is_valid(id) {
        backend.text_area_vim_state.get(&id).and_then(|state| {
            let NodeKind::TextArea(node) = &backend.core.tree.node(id).kind else {
                return None;
            };
            text_area_vim_search_feedback_for_text(state, node.value.as_ref(), cursor)
        })
    } else {
        None
    };
    if backend.core.tree.is_valid(id)
        && let NodeKind::TextArea(node) = &mut backend.core.tree.node_mut(id).kind
    {
        node.vim_mode = mode;
        node.vim_visual_line_caret = visual_line_caret;
        node.vim_search_feedback = search_feedback;
    }
}

pub(crate) trait MouseDispatchCtx<C: Component> {
    fn adjust_mouse(&mut self, mouse: MouseEvent) -> MouseEvent;
    fn mouse_state(&mut self) -> &mut crate::app::interaction_state::MouseTrackingState;
    fn drag_state(&mut self) -> &mut crate::app::interaction_state::DragState;
    fn tree(&self) -> &crate::core::node::NodeTree;
    fn tree_mut(&mut self) -> &mut crate::core::node::NodeTree;
    fn drag_is_active(&self) -> bool;
    fn active_drag(&self) -> ActiveDrag;

    fn dispatch_mouse_move(&mut self, mouse: MouseEvent) -> bool;
    fn update_hover(&mut self, x: u16, y: u16) -> bool;
    fn update_hover_impl(&mut self, x: u16, y: u16, force_recompute: bool) -> bool;
    fn dispatch_active_drag(&mut self, x: u16, y: u16) -> Option<bool>;
    fn handle_drag_release(&mut self, x: u16, y: u16) -> Option<bool>;
    fn handle_overlay_click(
        &mut self,
        _button: crate::core::event::MouseButton,
        _x: u16,
        _y: u16,
    ) -> bool {
        false
    }
    fn scroll_wheel_multiplier(&self) -> u16 {
        1
    }
    fn refresh_active_selection_drag_after_scroll(&mut self, _x: u16, _y: u16) -> bool {
        false
    }
    fn forward_terminal_mouse(&mut self, _mouse: MouseEvent) -> bool {
        false
    }
    fn handle_right_click_textarea(
        &mut self,
        hit: crate::core::node::NodeId,
        mouse: MouseEvent,
    ) -> bool;

    fn selection_owner_for_node(&self, start: crate::core::node::NodeId) -> Option<SelectionOwner>;
    fn clear_selectable_widget_selections(&mut self, keep: Option<SelectionOwner>) -> bool;

    fn focus_for_node(&mut self, id: crate::core::node::NodeId) -> bool;
    fn handle_scrollbar_click(
        &mut self,
        target: crate::core::node::ScrollbarTarget,
        x: u16,
        y: u16,
    ) -> bool;
    fn handle_slider_click(
        &mut self,
        hit: crate::core::node::NodeId,
        change: crate::app::input::mouse::SliderChange,
        x: u16,
        y: u16,
    ) -> bool;
    fn handle_progress_click(&mut self, change: crate::app::input::mouse::ProgressChange) -> bool;
    fn handle_draggable_tab_bar_click(
        &mut self,
        action: crate::app::input::mouse::DraggableTabBarAction,
        x: u16,
        dirty: bool,
    ) -> bool;
    fn handle_splitter_click(
        &mut self,
        grab: crate::app::input::mouse::SplitterGrab,
        x: u16,
        y: u16,
    ) -> bool;
    fn handle_list_click(
        &mut self,
        hit: crate::core::node::NodeId,
        select: crate::app::input::mouse::ListSelect,
        x: u16,
        y: u16,
    ) -> bool;
    fn handle_table_click(
        &mut self,
        hit: crate::core::node::NodeId,
        select: crate::app::input::mouse::TableSelect,
        x: u16,
        y: u16,
    ) -> bool;
    fn handle_terminal_click(
        &mut self,
        _hit: crate::core::node::NodeId,
        _mouse: MouseEvent,
        _x: u16,
        _y: u16,
        _hover_dirty: bool,
    ) -> bool {
        false
    }
    fn handle_textarea_click(
        &mut self,
        change: crate::app::input::mouse::TextAreaChange,
        x: u16,
        y: u16,
    ) -> bool;
    fn handle_document_view_click(
        &mut self,
        hit: crate::core::node::NodeId,
        mouse: MouseEvent,
        x: u16,
        y: u16,
    ) -> (bool, bool);
    fn handle_input_click(&mut self, change: crate::app::input::mouse::InputChange, x: u16)
    -> bool;
    fn handle_hex_area_click(
        &mut self,
        hit: crate::core::node::NodeId,
        mouse: MouseEvent,
        x: u16,
        y: u16,
    ) -> bool;

    fn handle_tabs_click(&mut self, change: crate::app::input::mouse::TabsChange) -> bool;
    fn handle_checkbox_click(&mut self, toggle: crate::app::input::mouse::CheckboxToggle) -> bool;
    fn handle_document_click(&mut self, click: crate::app::input::mouse::DocumentClick) -> bool;
    fn handle_graph_node_click(&mut self, click: crate::app::input::mouse::GraphNodeClick) -> bool;
    fn handle_sequence_item_click(
        &mut self,
        click: crate::app::input::mouse::SequenceItemClick,
    ) -> bool;
    fn handle_flowchart_item_click(
        &mut self,
        click: crate::app::input::mouse::FlowchartItemClick,
    ) -> bool;
    fn handle_fallback_click(
        &mut self,
        cb: crate::callback::Callback<MouseEvent>,
        mouse: MouseEvent,
    ) -> bool;
}

pub(crate) enum SelectionOwner {
    Node(crate::core::node::NodeId),
    DocumentShared {
        scroll_view_id: crate::core::node::NodeId,
        shared_selection_id: std::sync::Arc<str>,
    },
}
fn dispatch_mouse_shared<C: Component, T: MouseDispatchCtx<C>>(
    ctx: &mut T,
    mouse: MouseEvent,
) -> bool {
    let adjusted_mouse = ctx.adjust_mouse(mouse);
    let x = adjusted_mouse.x;
    let y = adjusted_mouse.y;
    let is_down = matches!(mouse.kind, MouseKind::Down(MouseButton::Left));
    let is_up = matches!(mouse.kind, MouseKind::Up(MouseButton::Left));

    transition_drag_threshold(ctx, &mouse, x, y);

    if let Some(result) = transition_mouse_region_drag(ctx, &mouse, x, y) {
        return result;
    }

    if let Some(result) = transition_pan_view_drag(ctx, &mouse, x, y) {
        return result;
    }

    if let Some(result) = transition_drag_move(ctx, adjusted_mouse, x, y) {
        return result;
    }

    if let Some(result) = transition_active_drag(ctx, &mouse, x, y) {
        return result;
    }

    let (drag_dirty, drag_consumed_up) = transition_drag_release(ctx, &mouse, x, y);
    if drag_consumed_up {
        return drag_dirty;
    }

    if matches!(mouse.kind, MouseKind::Up(_)) && ctx.forward_terminal_mouse(adjusted_mouse) {
        return true;
    }

    if let Some(result) = transition_overlay_click(ctx, &mouse, x, y) {
        return result;
    }

    if let Some(result) = transition_mouse_move(ctx, adjusted_mouse, x, y) {
        return result;
    }

    let has_active_drag = matches!(
        ctx.active_drag(),
        ActiveDrag::Slider(_)
            | ActiveDrag::Progress(_)
            | ActiveDrag::DragDrop(_)
            | ActiveDrag::Splitter(_)
            | ActiveDrag::Scrollbar(_)
            | ActiveDrag::TextArea(_)
            | ActiveDrag::DocumentView(_)
            | ActiveDrag::Input(_)
            | ActiveDrag::HexArea(_)
    );

    let should_update_hover = matches!(mouse.kind, MouseKind::Down(_))
        || (matches!(mouse.kind, MouseKind::Drag(_)) && has_active_drag);

    let mut hover_dirty = false;
    if should_update_hover {
        hover_dirty = ctx.update_hover(x, y);
    }

    if let Some(result) = transition_scroll_wheel(ctx, adjusted_mouse, x, y) {
        return result;
    }

    if let Some(result) = transition_right_click(ctx, adjusted_mouse, &mouse, x, y, hover_dirty) {
        return result;
    }

    if !is_down && !is_up {
        return hover_dirty;
    }

    let (hit_before_resolve, scrollbar_target) = transition_hit_test_and_scrollbar(ctx, x, y);

    let selection_dirty =
        transition_selection_clear(ctx, is_down, hit_before_resolve, scrollbar_target);

    if is_down {
        // A fresh button-down always begins a new interaction. Drop any in-flight
        // drag so a previous session whose button-up was lost cannot linger and
        // swallow this one (the Drag/Up arms ignore mismatched-button state without
        // clearing it, so the reset has to happen here). Left-drag is established
        // below; right-drag is handled earlier in `transition_right_click`.
        ctx.mouse_state().mouse_region_drag = None;

        // First pass, before the scrollbar/terminal/hit-miss early returns below and
        // before `resolve_left_click_target` (which may move the hit above the
        // draggable region): start from the raw deepest hit (or the scrollbar target)
        // so a drag that begins on those nodes is not lost. The post-resolve pass
        // further down refines this for the common path.
        let drag_start =
            hit_before_resolve.or_else(|| scrollbar_target.as_ref().map(|target| target.id));
        if let Some(start) = drag_start
            && let Some(target) = find_ancestor_mouse_region_drag_target(
                ctx.tree(),
                start,
                MouseButton::Left,
                mouse.mods,
                false,
            )
        {
            let origin_local =
                mouse_region_local_position(ctx.tree(), target, x, y).unwrap_or((0, 0));
            ctx.mouse_state().mouse_region_drag =
                Some(crate::app::interaction_state::MouseRegionDragState {
                    node_id: target,
                    button: MouseButton::Left,
                    origin: (x, y),
                    origin_local,
                    last_pos: (x, y),
                    started: false,
                });
        }
    }

    if is_down && ctx.forward_terminal_mouse(adjusted_mouse) {
        return true;
    }

    let hit =
        hit_before_resolve.map(|id| mouse::resolve_left_click_target(ctx.tree(), id, mouse.mods));

    if is_down
        && let Some(target) = scrollbar_target
        && ctx.handle_scrollbar_click(target, x, y)
    {
        return true;
    }

    let Some(hit) = hit else {
        if is_up {
            let state = ctx.mouse_state();
            state.left_down_node = None;
            state.left_down_pos = None;
            state.pending_drag_source = None;
        }
        if ctx.mouse_state().hovered.take().is_some() {
            return true;
        }
        return selection_dirty || hover_dirty;
    };

    if is_down {
        ctx.mouse_state().left_down_node = Some(hit);
    }

    ctx.mouse_state().hovered = mouse::should_hover(ctx.tree(), hit, x, y).then_some(hit);

    let mut dirty = selection_dirty;
    if is_down && ctx.focus_for_node(hit) {
        dirty = true;
    }
    if is_down
        && !ctx.tree().node(hit).is_focusable()
        && let Some(pan_id) = nearest_pan_view(ctx.tree(), hit)
        && ctx.focus_for_node(pan_id)
    {
        dirty = true;
    }

    if is_down {
        let actions = mouse::gather_hit_actions(ctx.tree(), hit, x, y);
        emit_bubbling_mouse_down(ctx.tree(), hit, adjusted_mouse);
        // Second pass (common path): refine the drag target from the resolved hit. If
        // nothing is found here the first-pass result is intentionally left in place.
        let drag_target = if mouse_region_has_drag_callbacks(&actions)
            && mouse_region_drag_target_accepts_mods(ctx.tree(), hit, MouseButton::Left, mouse.mods)
        {
            Some(hit)
        } else {
            find_ancestor_mouse_region_drag_target(
                ctx.tree(),
                hit,
                MouseButton::Left,
                mouse.mods,
                false,
            )
        };
        if let Some(target) = drag_target {
            let origin_local =
                mouse_region_local_position(ctx.tree(), target, x, y).unwrap_or((0, 0));
            ctx.mouse_state().mouse_region_drag =
                Some(crate::app::interaction_state::MouseRegionDragState {
                    node_id: target,
                    button: MouseButton::Left,
                    origin: (x, y),
                    origin_local,
                    last_pos: (x, y),
                    started: false,
                });
        }
        if let Some(pan_id) = nearest_pan_view(ctx.tree(), hit) {
            ctx.mouse_state().pan_view_drag =
                Some(crate::app::interaction_state::PanViewDragState {
                    node_id: pan_id,
                    last_pos: (x, y),
                    started: false,
                });
        }
        ctx.mouse_state().pending_drag_source =
            actions.drag_source_grab.as_ref().map(|g| g.node_id);
        if let Some(result) = transition_widget_down(
            ctx,
            WidgetDownParams {
                hit,
                mouse,
                adjusted_mouse,
                actions,
                x,
                y,
                hover_dirty,
            },
            &mut dirty,
        ) {
            return result;
        }
    }

    if is_up {
        let actions = mouse::gather_hit_actions(ctx.tree(), hit, x, y);
        if let Some(result) = transition_widget_up(ctx, hit, mouse, actions, x, y) {
            return result;
        }
    }

    dirty
}

#[cfg(not(target_arch = "wasm32"))]
impl<C: Component> MouseDispatchCtx<C> for AppRunner<C> {
    fn adjust_mouse(&mut self, mouse: MouseEvent) -> MouseEvent {
        let (x, y) = self.to_content_coords(mouse.x, mouse.y);
        MouseEvent { x, y, ..mouse }
    }

    fn mouse_state(&mut self) -> &mut crate::app::interaction_state::MouseTrackingState {
        &mut self.mouse
    }

    fn drag_state(&mut self) -> &mut crate::app::interaction_state::DragState {
        &mut self.drag
    }

    fn tree(&self) -> &crate::core::node::NodeTree {
        &self.core.tree
    }

    fn tree_mut(&mut self) -> &mut crate::core::node::NodeTree {
        &mut self.core.tree
    }

    fn drag_is_active(&self) -> bool {
        self.drag.is_active()
    }

    fn active_drag(&self) -> ActiveDrag {
        self.drag.active.clone()
    }

    fn dispatch_mouse_move(&mut self, mouse: MouseEvent) -> bool {
        AppRunner::<C>::dispatch_mouse_move(self, mouse)
    }

    fn update_hover(&mut self, x: u16, y: u16) -> bool {
        AppRunner::<C>::update_hover(self, x, y)
    }

    fn update_hover_impl(&mut self, x: u16, y: u16, force_recompute: bool) -> bool {
        AppRunner::<C>::update_hover_impl(self, x, y, force_recompute)
    }

    fn dispatch_active_drag(&mut self, x: u16, y: u16) -> Option<bool> {
        AppRunner::<C>::dispatch_active_drag(self, x, y)
    }

    fn handle_drag_release(&mut self, x: u16, y: u16) -> Option<bool> {
        AppRunner::<C>::handle_drag_release(self, x, y)
    }

    fn handle_overlay_click(
        &mut self,
        button: crate::core::event::MouseButton,
        x: u16,
        y: u16,
    ) -> bool {
        AppRunner::<C>::handle_overlay_click(self, button, x, y)
    }

    fn scroll_wheel_multiplier(&self) -> u16 {
        self.scroll_wheel_multiplier
    }

    fn refresh_active_selection_drag_after_scroll(&mut self, x: u16, y: u16) -> bool {
        AppRunner::<C>::refresh_active_selection_drag_at(self, x, y)
    }

    fn forward_terminal_mouse(&mut self, mouse: MouseEvent) -> bool {
        #[cfg(feature = "terminal")]
        {
            self.forward_terminal_mouse(mouse)
        }
        #[cfg(not(feature = "terminal"))]
        {
            let _ = mouse;
            false
        }
    }

    fn handle_right_click_textarea(
        &mut self,
        hit: crate::core::node::NodeId,
        mouse: MouseEvent,
    ) -> bool {
        AppRunner::<C>::handle_right_click_textarea(self, hit, mouse)
    }

    fn selection_owner_for_node(&self, start: crate::core::node::NodeId) -> Option<SelectionOwner> {
        use crate::app::runner::events::SelectionOwner as RunnerSelectionOwner;

        AppRunner::<C>::selection_owner_for_node(self, start).map(|owner| match owner {
            RunnerSelectionOwner::Node(id) => SelectionOwner::Node(id),
            RunnerSelectionOwner::DocumentShared {
                scroll_view_id,
                shared_selection_id,
            } => SelectionOwner::DocumentShared {
                scroll_view_id,
                shared_selection_id,
            },
        })
    }

    fn clear_selectable_widget_selections(&mut self, keep: Option<SelectionOwner>) -> bool {
        use crate::app::runner::events::SelectionOwner as RunnerSelectionOwner;

        let keep = keep.map(|owner| match owner {
            SelectionOwner::Node(id) => RunnerSelectionOwner::Node(id),
            SelectionOwner::DocumentShared {
                scroll_view_id,
                shared_selection_id,
            } => RunnerSelectionOwner::DocumentShared {
                scroll_view_id,
                shared_selection_id,
            },
        });
        AppRunner::<C>::clear_selectable_widget_selections(self, keep)
    }

    fn focus_for_node(&mut self, id: crate::core::node::NodeId) -> bool {
        AppRunner::<C>::focus_for_node(self, id)
    }

    fn handle_scrollbar_click(
        &mut self,
        target: crate::core::node::ScrollbarTarget,
        x: u16,
        y: u16,
    ) -> bool {
        AppRunner::<C>::handle_scrollbar_click(self, target, x, y)
    }

    fn handle_slider_click(
        &mut self,
        hit: crate::core::node::NodeId,
        change: crate::app::input::mouse::SliderChange,
        x: u16,
        y: u16,
    ) -> bool {
        AppRunner::<C>::handle_slider_click(self, hit, change, x, y)
    }

    fn handle_progress_click(&mut self, change: crate::app::input::mouse::ProgressChange) -> bool {
        AppRunner::<C>::handle_progress_click(self, change)
    }

    fn handle_draggable_tab_bar_click(
        &mut self,
        action: crate::app::input::mouse::DraggableTabBarAction,
        x: u16,
        dirty: bool,
    ) -> bool {
        AppRunner::<C>::handle_draggable_tab_bar_click(self, action, x, dirty)
    }

    fn handle_splitter_click(
        &mut self,
        grab: crate::app::input::mouse::SplitterGrab,
        x: u16,
        y: u16,
    ) -> bool {
        AppRunner::<C>::handle_splitter_click(self, grab, x, y)
    }

    fn handle_list_click(
        &mut self,
        hit: crate::core::node::NodeId,
        select: crate::app::input::mouse::ListSelect,
        x: u16,
        y: u16,
    ) -> bool {
        AppRunner::<C>::handle_list_click(self, hit, select, x, y)
    }

    fn handle_table_click(
        &mut self,
        hit: crate::core::node::NodeId,
        select: crate::app::input::mouse::TableSelect,
        x: u16,
        y: u16,
    ) -> bool {
        AppRunner::<C>::handle_table_click(self, hit, select, x, y)
    }

    fn handle_terminal_click(
        &mut self,
        hit: crate::core::node::NodeId,
        mouse: MouseEvent,
        x: u16,
        y: u16,
        hover_dirty: bool,
    ) -> bool {
        #[cfg(feature = "terminal")]
        {
            AppRunner::<C>::handle_terminal_click(self, hit, mouse, x, y, hover_dirty)
        }
        #[cfg(not(feature = "terminal"))]
        {
            let _ = (hit, mouse, x, y, hover_dirty);
            false
        }
    }

    fn handle_textarea_click(
        &mut self,
        change: crate::app::input::mouse::TextAreaChange,
        x: u16,
        y: u16,
    ) -> bool {
        AppRunner::<C>::handle_textarea_click(self, change, x, y)
    }

    fn handle_document_view_click(
        &mut self,
        hit: crate::core::node::NodeId,
        mouse: MouseEvent,
        x: u16,
        y: u16,
    ) -> (bool, bool) {
        AppRunner::<C>::handle_document_view_click(self, hit, mouse, x, y)
    }

    fn handle_input_click(
        &mut self,
        change: crate::app::input::mouse::InputChange,
        x: u16,
    ) -> bool {
        AppRunner::<C>::handle_input_click(self, change, x)
    }

    fn handle_hex_area_click(
        &mut self,
        hit: crate::core::node::NodeId,
        mouse: MouseEvent,
        x: u16,
        y: u16,
    ) -> bool {
        AppRunner::<C>::handle_hex_area_click(self, hit, mouse, x, y)
    }

    fn handle_tabs_click(&mut self, change: crate::app::input::mouse::TabsChange) -> bool {
        AppRunner::<C>::handle_tabs_click(self, change)
    }

    fn handle_checkbox_click(&mut self, toggle: crate::app::input::mouse::CheckboxToggle) -> bool {
        AppRunner::<C>::handle_checkbox_click(self, toggle)
    }

    fn handle_document_click(&mut self, click: crate::app::input::mouse::DocumentClick) -> bool {
        AppRunner::<C>::handle_document_click_event(self, click)
    }

    fn handle_graph_node_click(&mut self, click: crate::app::input::mouse::GraphNodeClick) -> bool {
        AppRunner::<C>::handle_graph_node_click(self, click)
    }

    fn handle_sequence_item_click(
        &mut self,
        click: crate::app::input::mouse::SequenceItemClick,
    ) -> bool {
        AppRunner::<C>::handle_sequence_item_click(self, click)
    }

    fn handle_flowchart_item_click(
        &mut self,
        click: crate::app::input::mouse::FlowchartItemClick,
    ) -> bool {
        AppRunner::<C>::handle_flowchart_item_click(self, click)
    }

    fn handle_fallback_click(
        &mut self,
        cb: crate::callback::Callback<MouseEvent>,
        mouse: MouseEvent,
    ) -> bool {
        AppRunner::<C>::handle_fallback_on_click(self, cb, mouse)
    }
}

impl<C: Component> MouseDispatchCtx<C> for TestBackend<C> {
    fn adjust_mouse(&mut self, mouse: MouseEvent) -> MouseEvent {
        mouse
    }

    fn mouse_state(&mut self) -> &mut crate::app::interaction_state::MouseTrackingState {
        &mut self.mouse
    }

    fn drag_state(&mut self) -> &mut crate::app::interaction_state::DragState {
        &mut self.drag
    }

    fn tree(&self) -> &crate::core::node::NodeTree {
        &self.core.tree
    }

    fn tree_mut(&mut self) -> &mut crate::core::node::NodeTree {
        &mut self.core.tree
    }

    fn drag_is_active(&self) -> bool {
        self.drag.is_active()
    }

    fn active_drag(&self) -> ActiveDrag {
        self.drag.active.clone()
    }

    fn dispatch_mouse_move(&mut self, mouse: MouseEvent) -> bool {
        dispatch_mouse_move_test_backend(self, mouse)
    }

    fn update_hover(&mut self, x: u16, y: u16) -> bool {
        update_hover_test_backend(self, x, y, false)
    }

    fn update_hover_impl(&mut self, x: u16, y: u16, force_recompute: bool) -> bool {
        update_hover_test_backend(self, x, y, force_recompute)
    }

    fn dispatch_active_drag(&mut self, x: u16, y: u16) -> Option<bool> {
        dispatch_active_drag_test_backend(self, x, y)
    }

    fn handle_drag_release(&mut self, x: u16, y: u16) -> Option<bool> {
        if matches!(self.drag.active, ActiveDrag::None) {
            return None;
        }
        // Mirror of the AppRunner DragDrop release arm, without the snapshot
        // preview cache (not used by the headless backend).
        if let ActiveDrag::DragDrop(drag_state) = self.drag.active.clone() {
            let payload = drag_state.payload.clone();

            if let Some(target_id) = drag_state.hovered_target
                && self.core.tree.is_valid(target_id)
                && matches!(self.core.tree.node(target_id).kind, NodeKind::DropTarget(_))
            {
                let rect = self.core.tree.node(target_id).rect;
                let top = rect.y.max(0) as u16;
                let local_y = y.saturating_sub(top);
                if let NodeKind::DropTarget(target) = &mut self.core.tree.node_mut(target_id).kind {
                    target.dnd_highlighted = false;
                    if let Some(cb) = &target.on_drop {
                        cb.emit(crate::widgets::DropEvent {
                            x,
                            y,
                            local_y,
                            local_height: rect.h,
                            payload: payload.clone(),
                        });
                    }
                    if let Some(cb) = &target.on_drag_leave {
                        cb.emit(crate::widgets::DragLeaveEvent { payload });
                    }
                }
            } else if let Some(cb) = drag_state.on_cancel {
                cb.emit(crate::widgets::DragCancelEvent { payload });
            }

            crate::app::input::drag::set_drag_source_dragging(
                &mut self.core.tree,
                drag_state.source_id,
                false,
            );
            self.drag.clear();
            return Some(true);
        }
        if let ActiveDrag::TextArea(drag_state) = self.drag.active.clone()
            && self.core.tree.is_valid(drag_state.id)
            && let NodeKind::TextArea(node) = &self.core.tree.node(drag_state.id).kind
            && let Some(cb) = node.on_change.clone()
        {
            cb.emit(crate::widgets::TextAreaEvent {
                value: node.value.clone(),
                cursor: node.cursor,
                anchor: node.anchor,
            });
        }
        self.drag.clear();
        Some(true)
    }

    fn handle_right_click_textarea(
        &mut self,
        hit: crate::core::node::NodeId,
        mouse: MouseEvent,
    ) -> bool {
        if let NodeKind::TextArea(text_area) = &self.core.tree.node(hit).kind
            && !text_area.disabled
            && let Some(cb) = text_area.on_click.clone()
        {
            cb.emit(mouse);
            return true;
        }
        false
    }

    fn selection_owner_for_node(&self, start: crate::core::node::NodeId) -> Option<SelectionOwner> {
        selection_owner_for_node_shared(&self.core.tree, start)
    }

    fn clear_selectable_widget_selections(&mut self, keep: Option<SelectionOwner>) -> bool {
        clear_selectable_widget_selections_test_backend(self, keep)
    }

    fn focus_for_node(&mut self, id: crate::core::node::NodeId) -> bool {
        TestBackend::<C>::focus_for_node(self, id)
    }

    fn handle_scrollbar_click(
        &mut self,
        target: crate::core::node::ScrollbarTarget,
        x: u16,
        y: u16,
    ) -> bool {
        handle_scrollbar_click_test_backend(self, target, x, y)
    }

    fn handle_slider_click(
        &mut self,
        _hit: crate::core::node::NodeId,
        change: crate::app::input::mouse::SliderChange,
        x: u16,
        y: u16,
    ) -> bool {
        if let Some((value, on_change, on_click)) =
            drag::handle_slider_drag(&self.core.tree, x, y, change.node_id, true)
        {
            self.drag.active =
                ActiveDrag::Slider(crate::app::input::drag::SliderDrag { id: change.node_id });
            if let Some(cb) = on_change {
                cb.emit(value);
            }
            if let Some(cb) = on_click {
                cb.emit(value);
            }
            return true;
        }
        false
    }

    fn handle_progress_click(&mut self, change: crate::app::input::mouse::ProgressChange) -> bool {
        if change.draggable {
            self.drag.active =
                ActiveDrag::Progress(crate::app::input::drag::ProgressDrag { id: change.node_id });
        }
        if let Some(cb) = change.on_change {
            cb.emit(crate::widgets::ProgressEvent {
                progress: change.progress,
            });
            return true;
        }
        false
    }

    fn handle_draggable_tab_bar_click(
        &mut self,
        action: crate::app::input::mouse::DraggableTabBarAction,
        x: u16,
        dirty: bool,
    ) -> bool {
        handle_draggable_tab_bar_click_test_backend(self, action, x, dirty)
    }

    fn handle_splitter_click(
        &mut self,
        grab: crate::app::input::mouse::SplitterGrab,
        x: u16,
        y: u16,
    ) -> bool {
        handle_splitter_click_test_backend(self, grab, x, y)
    }

    fn handle_list_click(
        &mut self,
        hit: crate::core::node::NodeId,
        select: crate::app::input::mouse::ListSelect,
        x: u16,
        y: u16,
    ) -> bool {
        handle_list_click_test_backend(self, hit, select, x, y)
    }

    fn handle_table_click(
        &mut self,
        hit: crate::core::node::NodeId,
        select: crate::app::input::mouse::TableSelect,
        x: u16,
        y: u16,
    ) -> bool {
        handle_table_click_test_backend(self, hit, select, x, y)
    }

    fn handle_textarea_click(
        &mut self,
        change: crate::app::input::mouse::TextAreaChange,
        x: u16,
        y: u16,
    ) -> bool {
        if change.on_change.is_none() && !change.read_only {
            return false;
        }

        let is_active = Some(change.node_id) == self.focused
            || !change.focusable
            || self.focus_policy == crate::FocusPolicy::Manual;
        if is_active {
            let (new_cursor, new_anchor, anchor_for_drag) =
                mouse::process_textarea_click(&change, x, y, &mut self.mouse.last_click);
            self.drag.last_pointer_pos = None;
            self.drag.last_autoscroll_tick = None;
            self.drag.autoscroll_layout_dirty = false;
            self.drag.active = ActiveDrag::TextArea(crate::app::input::drag::TextAreaDrag {
                id: change.node_id,
                anchor: anchor_for_drag,
            });
            if let NodeKind::TextArea(node) = &mut self.core.tree.node_mut(change.node_id).kind {
                node.cursor = new_cursor;
                node.anchor = new_anchor;
            }
            sync_textarea_vim_external_selection(
                self,
                TextareaVimExternalSelectionParams {
                    id: change.node_id,
                    vim_motions: change.vim_motions,
                    read_only: change.read_only,
                    has_on_change: change.on_change.is_some(),
                    on_vim_mode_change: change.on_vim_mode_change.as_ref(),
                    cursor: new_cursor,
                    anchor: new_anchor,
                },
            );
            if let Some(cb) = change.on_change.as_ref() {
                cb.emit(crate::widgets::TextAreaEvent {
                    value: change.value.clone(),
                    cursor: new_cursor,
                    anchor: new_anchor,
                });
            } else if change.read_only {
                self.read_only_selection
                    .insert(change.node_id, (new_cursor, new_anchor));
            }
            return true;
        }
        false
    }

    fn handle_document_view_click(
        &mut self,
        hit: crate::core::node::NodeId,
        _mouse: MouseEvent,
        x: u16,
        y: u16,
    ) -> (bool, bool) {
        (
            false,
            handle_document_view_click_test_backend(self, hit, x, y),
        )
    }

    fn handle_input_click(
        &mut self,
        change: crate::app::input::mouse::InputChange,
        x: u16,
    ) -> bool {
        let is_active = Some(change.node_id) == self.focused
            || !change.focusable
            || self.focus_policy == crate::FocusPolicy::Manual;
        if is_active {
            let (new_cursor, new_anchor, anchor_for_drag) =
                mouse::process_input_click(&change, x, &mut self.mouse.last_click);
            self.drag.active = ActiveDrag::Input(crate::app::input::drag::InputDrag {
                id: change.node_id,
                anchor: anchor_for_drag,
            });
            if let Some(cb) = change.on_change {
                cb.emit(crate::widgets::InputEvent {
                    value: change.value,
                    cursor: new_cursor,
                    anchor: new_anchor,
                });
            } else if change.read_only {
                self.read_only_selection
                    .insert(change.node_id, (new_cursor, new_anchor));
            }
            return true;
        }
        false
    }

    fn handle_hex_area_click(
        &mut self,
        hit: crate::core::node::NodeId,
        mouse: MouseEvent,
        x: u16,
        y: u16,
    ) -> bool {
        handle_hex_area_click_test_backend(self, hit, mouse, x, y)
    }

    fn handle_tabs_click(&mut self, change: crate::app::input::mouse::TabsChange) -> bool {
        if change.next != change.active {
            change
                .cb
                .emit(crate::widgets::TabsEvent { index: change.next });
            return true;
        }
        false
    }

    fn handle_checkbox_click(&mut self, toggle: crate::app::input::mouse::CheckboxToggle) -> bool {
        toggle.cb.emit(crate::widgets::CheckboxEvent {
            state: toggle.state.toggle(),
        });
        true
    }

    fn handle_document_click(&mut self, click: crate::app::input::mouse::DocumentClick) -> bool {
        click.cb.emit(crate::widgets::DocumentClickEvent {
            source_line: click.source_line,
            link: click.link,
        });
        true
    }

    fn handle_graph_node_click(&mut self, click: crate::app::input::mouse::GraphNodeClick) -> bool {
        let focus_cb = {
            let NodeKind::Graph(graph) = &mut self.core.tree.node_mut(click.node_id).kind else {
                return false;
            };
            graph
                .set_focused_path(click.event.path.clone())
                .then(|| graph.on_node_focus.clone())
                .flatten()
        };

        if let Some(cb) = focus_cb.as_ref() {
            cb.emit(click.event.clone());
        }
        if let Some(cb) = click.cb {
            cb.emit(click.event);
            true
        } else {
            focus_cb.is_some()
        }
    }

    fn handle_sequence_item_click(
        &mut self,
        click: crate::app::input::mouse::SequenceItemClick,
    ) -> bool {
        click.cb.emit(click.event);
        true
    }

    fn handle_flowchart_item_click(
        &mut self,
        click: crate::app::input::mouse::FlowchartItemClick,
    ) -> bool {
        match click {
            crate::app::input::mouse::FlowchartItemClick::Node { cb, event } => cb.emit(event),
            crate::app::input::mouse::FlowchartItemClick::Edge { cb, event } => cb.emit(event),
            crate::app::input::mouse::FlowchartItemClick::Subgraph { cb, event } => cb.emit(event),
        }
        true
    }

    fn handle_fallback_click(
        &mut self,
        cb: crate::callback::Callback<MouseEvent>,
        mouse: MouseEvent,
    ) -> bool {
        cb.emit(mouse);
        true
    }
}

fn selection_owner_for_node_shared(
    tree: &crate::core::node::NodeTree,
    start: crate::core::node::NodeId,
) -> Option<SelectionOwner> {
    let mut cur = Some(start);
    while let Some(id) = cur {
        if !tree.is_valid(id) {
            break;
        }
        let node = tree.node(id);
        match &node.kind {
            NodeKind::Input(_) | NodeKind::TextArea(_) | NodeKind::HexArea(_) => {
                return Some(SelectionOwner::Node(id));
            }
            NodeKind::DocumentView(doc) => {
                if let Some(shared_selection_id) = doc.shared_selection_id.clone()
                    && let Some(scroll_view_id) = drag::nearest_ancestor_scroll_view(tree, id)
                {
                    return Some(SelectionOwner::DocumentShared {
                        scroll_view_id,
                        shared_selection_id,
                    });
                }
                return Some(SelectionOwner::Node(id));
            }
            _ => {
                cur = node.parent;
            }
        }
    }
    None
}

#[cfg(test)]
mod tests;
