use crate::animation::Transition;
use crate::core::component::FocusContext;
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::measure::min_size_constrained;
use crate::layout::reconcile::{
    OverlayState, ReconcileCtx, SingleChildReconcile, reconcile_single_child,
};
use crate::style::{Length, Rect};

use super::{Animated, AnimatedNode};

pub(crate) fn reconcile_animated(
    tree: &mut NodeTree,
    epoch: u32,
    id: NodeId,
    animated: &Animated,
    rect: Rect,
    focus: Option<&FocusContext>,
    overlay_state: &mut OverlayState,
) -> NodeId {
    let mut child_rect = rect;
    let (old_children, height_animating) = {
        let node = tree.node_mut(id);
        let old_rect = node.rect;
        let was_animated = matches!(node.kind, NodeKind::Animated(_));

        let mut next = if let NodeKind::Animated(existing) = &node.kind {
            existing.clone()
        } else {
            AnimatedNode::from(animated.clone())
        };

        let target_opacity = animated.opacity.clamp(0.0, 1.0);
        if (next.target_opacity - target_opacity).abs() > f32::EPSILON {
            next.target_opacity = target_opacity;
            if animated.transition.duration.is_zero() {
                next.opacity = target_opacity;
                next.opacity_anim = None;
                if let Some(cb) = &animated.on_opacity_transition_end {
                    cb.emit(());
                }
            } else {
                next.opacity_anim = Some(Transition::new(
                    next.opacity,
                    target_opacity,
                    animated.transition.duration,
                    animated.transition.easing,
                ));
            }
        }

        if next.target_fg != animated.fg {
            next.target_fg = animated.fg;
            match next.target_fg {
                Some(target) if !animated.transition.duration.is_zero() => {
                    let start = next.current_fg.unwrap_or(crate::style::Color::Reset);
                    next.fg_anim = Some(Transition::new(
                        start,
                        target,
                        animated.transition.duration,
                        animated.transition.easing,
                    ));
                }
                Some(target) => {
                    next.current_fg = Some(target);
                    next.fg_anim = None;
                }
                None => {
                    // Clearing explicit override should settle immediately to inherited child color.
                    next.current_fg = None;
                    next.fg_anim = None;
                }
            }
        }

        if next.target_bg != animated.bg {
            next.target_bg = animated.bg;
            match next.target_bg {
                Some(target) if !animated.transition.duration.is_zero() => {
                    let start = next.current_bg.unwrap_or(crate::style::Color::Reset);
                    next.bg_anim = Some(Transition::new(
                        start,
                        target,
                        animated.transition.duration,
                        animated.transition.easing,
                    ));
                }
                Some(target) => {
                    next.current_bg = Some(target);
                    next.bg_anim = None;
                }
                None => {
                    // Clearing explicit override should settle immediately to inherited child color.
                    next.current_bg = None;
                    next.bg_anim = None;
                }
            }
        }

        next.transition_easing = animated.transition.easing;
        next.transition_duration = animated.transition.duration;
        next.opacity_fg_only = animated.opacity_fg_only;
        next.opacity_target = animated.opacity_target;
        next.position_transition = animated.position_transition;

        let (_, natural_h) = min_size_constrained(animated.child.as_ref(), Some(rect.w), None);
        let next_target_height = animated.height.map(|height| match height {
            Length::Auto => natural_h,
            Length::Px(px) => px,
            Length::Percent(percent) => Length::Percent(percent).resolve(rect.h, natural_h),
            Length::Flex(_) => rect.h,
        });

        if let Some(target_height) = next_target_height {
            if next.target_height != Some(target_height) {
                let current_height = next
                    .height_anim
                    .as_ref()
                    .map(|transition| transition.current().round().max(0.0) as u16)
                    .or(next.prev_height)
                    .or(next.target_height)
                    .unwrap_or(rect.h);
                next.target_height = Some(target_height);
                if animated.transition.duration.is_zero() {
                    next.prev_height = Some(target_height);
                    next.height_anim = None;
                    if let Some(cb) = &animated.on_height_transition_end {
                        cb.emit(());
                    }
                } else {
                    next.prev_height = Some(current_height);
                    next.height_anim = Some(Transition::new(
                        current_height as f32,
                        target_height as f32,
                        animated.transition.duration,
                        animated.transition.easing,
                    ));
                }
            } else if next.prev_height.is_none() {
                next.prev_height = Some(target_height);
            }
        } else {
            next.height_anim = None;
            next.prev_height = None;
            next.target_height = None;
        }

        next.on_opacity_transition_end = animated.on_opacity_transition_end.clone();
        next.on_height_transition_end = animated.on_height_transition_end.clone();
        next.on_position_transition_end = animated.on_position_transition_end.clone();

        let position_changed = old_rect.x != rect.x || old_rect.y != rect.y;
        if was_animated && animated.position_transition && position_changed {
            let current_visual_x = old_rect.x as f32 + next.current_x_offset;
            let current_visual_y = old_rect.y as f32 + next.current_y_offset;
            let start_x_offset = current_visual_x - rect.x as f32;
            let start_y_offset = current_visual_y - rect.y as f32;

            if start_x_offset.abs() > 0.001 || start_y_offset.abs() > 0.001 {
                if animated.transition.duration.is_zero() {
                    next.current_x_offset = 0.0;
                    next.current_y_offset = 0.0;
                    next.x_position_anim = None;
                    next.y_position_anim = None;
                    if let Some(cb) = &animated.on_position_transition_end {
                        cb.emit(());
                    }
                } else {
                    next.current_x_offset = start_x_offset;
                    next.current_y_offset = start_y_offset;
                    next.x_position_anim = Some(Transition::new(
                        start_x_offset,
                        0.0,
                        animated.transition.duration,
                        animated.transition.easing,
                    ));
                    next.y_position_anim = Some(Transition::new(
                        start_y_offset,
                        0.0,
                        animated.transition.duration,
                        animated.transition.easing,
                    ));
                }
            } else {
                let was_position_animating =
                    next.x_position_anim.is_some() || next.y_position_anim.is_some();
                next.current_x_offset = 0.0;
                next.current_y_offset = 0.0;
                next.x_position_anim = None;
                next.y_position_anim = None;
                if was_position_animating && let Some(cb) = &animated.on_position_transition_end {
                    cb.emit(());
                }
            }
        } else if !animated.position_transition {
            next.current_x_offset = 0.0;
            next.current_y_offset = 0.0;
            next.x_position_anim = None;
            next.y_position_anim = None;
        }

        let raw_visible = next.current_visible_height(rect.h);
        let visible_h = if rect.h == 0 {
            raw_visible
        } else {
            raw_visible.min(rect.h)
        };
        child_rect.h = visible_h;
        let height_animating = next.height_anim.is_some();

        node.kind = NodeKind::Animated(next);
        let mut assigned = rect;
        assigned.h = visible_h;
        node.rect = assigned;
        (std::mem::take(&mut node.children), height_animating)
    };

    let child_for_reconcile = if height_animating {
        animated
            .child
            .as_ref()
            .clone()
            .max_height(Length::Px(child_rect.h))
    } else {
        animated.child.as_ref().clone()
    };

    let new_children = reconcile_single_child(
        &mut ReconcileCtx {
            tree,
            epoch,
            focus,
            overlay_state,
        },
        SingleChildReconcile {
            parent_id: id,
            child: Some(&child_for_reconcile),
            rect: child_rect,
            old_children,
        },
    );

    tree.node_mut(id).children = new_children;
    id
}
