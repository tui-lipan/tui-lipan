#[cfg(feature = "image")]
#[cfg(feature = "image")]
use crate::backend::ratatui_backend::image_support;
use crate::core::component::Component;
use crate::core::node::NodeKind;
#[cfg(feature = "image")]
use crate::style::Rect;
#[cfg(feature = "image")]
use std::collections::hash_map::DefaultHasher;
#[cfg(feature = "image")]
use std::hash::{Hash, Hasher};
#[cfg(any(test, feature = "image"))]
use std::time::Duration;

use super::{AppRunner, spinner_frame_for_speed};

/// Advance one auto-animating list spinner slot to the shared animation tick.
fn advance_list_spinner(
    spinner: Option<&mut crate::widgets::list::ListItemSpinnerSlot>,
    tick: usize,
    next_tick: &mut usize,
) -> bool {
    if let Some(spinner) = spinner.filter(|spinner| spinner.auto_frame) {
        return advance_spinner_frame(
            &mut spinner.frame,
            tick,
            spinner.speed,
            spinner.spinner_style,
            next_tick,
        );
    }
    false
}

/// Compare the glyph, not the unbounded tick counter (including a complete cycle after a stall).
fn advance_spinner_frame(
    frame: &mut usize,
    tick: usize,
    speed: crate::widgets::SpinnerSpeed,
    style: crate::widgets::SpinnerStyle,
    next_tick: &mut usize,
) -> bool {
    *next_tick = (*next_tick).min(next_spinner_frame_tick(tick, speed));
    let next = spinner_frame_for_speed(tick, speed);
    let frames = style.frames();
    let changed = match style {
        // These styles animate position and color in a procedural renderer. Their fallback
        // `frames()` strings are identical, but their rendered frames are not.
        crate::widgets::SpinnerStyle::OpenCode | crate::widgets::SpinnerStyle::Lightsaber => {
            *frame % frames.len() != next % frames.len()
        }
        _ => frames[*frame % frames.len()] != frames[next % frames.len()],
    };
    *frame = next;
    changed
}

/// Quantize the next frame boundary to the shared 50 ms clock, without waking on unused ticks.
fn next_spinner_frame_tick(tick: usize, speed: crate::widgets::SpinnerSpeed) -> usize {
    let frame_ms = match speed {
        crate::widgets::SpinnerSpeed::Fast => 50,
        crate::widgets::SpinnerSpeed::Normal => 100,
        crate::widgets::SpinnerSpeed::Slow => 200,
        crate::widgets::SpinnerSpeed::Custom { frame_ms } => usize::from(frame_ms).max(1),
    };
    let next_frame = spinner_frame_for_speed(tick, speed).saturating_add(1);
    next_frame
        .saturating_mul(frame_ms)
        .div_ceil(50)
        .max(tick.saturating_add(1))
}

fn advance_tab_spinners(
    node: &mut crate::widgets::draggable_tab_bar::DraggableTabBarNode,
    tick: usize,
    next_tick: &mut usize,
) -> bool {
    let mut changed = false;
    for tab in std::sync::Arc::make_mut(&mut node.tabs) {
        if let Some(spinner) = tab
            .leading
            .as_mut()
            .and_then(|content| content.spinner_mut())
            && spinner.auto_frame
        {
            let mut frame = spinner.spinner.frame.unwrap_or(0);
            changed |= advance_spinner_frame(
                &mut frame,
                tick,
                spinner.spinner.speed,
                spinner.spinner.spinner_style,
                next_tick,
            );
            spinner.spinner.frame = Some(frame);
        }
    }
    changed
}

fn advance_list_spinners(
    node: &mut crate::widgets::list::ListNode,
    tick: usize,
    next_tick: &mut usize,
) -> bool {
    let mut changed = false;
    for item in std::sync::Arc::make_mut(&mut node.items) {
        changed |= advance_list_spinner(
            item.status.as_mut().and_then(|s| s.spinner_mut()),
            tick,
            next_tick,
        );
        changed |= advance_list_spinner(
            item.gutter.as_mut().and_then(|g| g.spinner_mut()),
            tick,
            next_tick,
        );
        changed |= advance_list_spinner(item.description_spinner.as_mut(), tick, next_tick);
        changed |= advance_list_spinner(item.label_spinner.as_mut(), tick, next_tick);
        for line in &mut item.extra_lines {
            changed |= advance_list_spinner(line.description_spinner.as_mut(), tick, next_tick);
            changed |= advance_list_spinner(line.label_spinner.as_mut(), tick, next_tick);
        }
    }
    changed
}

fn advance_node_spinners(kind: &mut NodeKind, tick: usize, next_tick: &mut usize) -> bool {
    match kind {
        NodeKind::Spinner(node) if node.auto_frame => advance_spinner_frame(
            &mut node.frame,
            tick,
            node.speed,
            node.spinner_style,
            next_tick,
        ),
        NodeKind::DraggableTabBar(node) => advance_tab_spinners(node, tick, next_tick),
        NodeKind::List(node) => advance_list_spinners(node, tick, next_tick),
        _ => false,
    }
}

impl<C: Component> AppRunner<C> {
    pub(crate) fn update_spinner_frames(&mut self) -> bool {
        let spinner_ids = self.core.tree.spinner_ids().to_vec();
        let mut changed = false;
        let mut next_tick = usize::MAX;
        for id in spinner_ids {
            if advance_node_spinners(
                &mut self.core.tree.node_mut(id).kind,
                self.animation.spinner_frame,
                &mut next_tick,
            ) {
                changed = true;
                if !self.animation.spinner_damage.contains(&id) {
                    self.animation.spinner_damage.push(id);
                }
            }
        }
        let ticks = next_tick
            .saturating_sub(self.animation.spinner_frame)
            .max(1);
        self.animation.spinner_tick_interval =
            std::time::Duration::from_millis((ticks as u64).saturating_mul(50));
        changed
    }

    #[cfg(test)]
    pub(crate) fn update_animated_widgets(&mut self, dt: Duration) -> (bool, bool, bool) {
        crate::app::animation::tick_animated_widgets(&mut self.core.tree, dt)
    }

    #[cfg(test)]
    pub(crate) fn update_smooth_scrolls(&mut self, dt: Duration) -> (bool, bool, bool) {
        crate::app::animation::tick_smooth_scrolls(&mut self.core.tree, dt)
    }

    #[cfg(feature = "image")]
    pub(crate) fn update_image_frames(&mut self, delta_ms: u32) -> bool {
        if self.surface.is_inline() {
            return false;
        }

        let Some(viewport) = self.image_animation_viewport() else {
            return false;
        };

        let image_ids = self.core.tree.animated_image_ids().to_vec();

        let mut any_advanced = false;
        for id in image_ids {
            if self
                .core
                .tree
                .node(id)
                .rect
                .intersection(&viewport)
                .is_empty()
            {
                continue;
            }
            if let NodeKind::Image(node) = &mut self.core.tree.node_mut(id).kind
                && node.tick_animation(delta_ms)
            {
                any_advanced = true;
            }
        }

        any_advanced
    }

    #[cfg(feature = "image")]
    pub(crate) fn next_image_frame_due_in_ms(&self) -> Option<u32> {
        if self.surface.is_inline() {
            return None;
        }

        let viewport = self.image_animation_viewport()?;

        self.core
            .tree
            .animated_image_ids()
            .iter()
            .filter_map(|&id| {
                let node = self.core.tree.node(id);
                if node.rect.intersection(&viewport).is_empty() {
                    return None;
                }
                match &node.kind {
                    NodeKind::Image(image) => image.next_frame_due_in_ms(),
                    _ => None,
                }
            })
            .min()
    }

    #[cfg(feature = "image")]
    pub(crate) fn image_animations_suspended(&self) -> bool {
        self.animation
            .image_animation_suspend_until
            .is_some_and(|deadline| self.core.ctx.env().now() < deadline)
    }

    #[cfg(feature = "image")]
    pub(super) fn pause_images_after_resize(&mut self) {
        let pause = Duration::from_millis(u64::from(super::image_resize_pause_ms()));
        if pause.is_zero() {
            return;
        }
        // Opt-in throttling for hosts where resize uploads are expensive. A fixed pause must
        // not delay live terminal pictures on every resize by default.
        self.suspend_image_animations_for(pause);
        image_support::suspend_image_rendering_for(pause);
    }

    #[cfg(feature = "image")]
    pub(crate) fn suspend_image_animations_for(&mut self, duration: Duration) {
        let now = self.core.ctx.env().now();
        let requested_deadline = now + duration;
        self.animation.image_animation_suspend_until = Some(
            self.animation
                .image_animation_suspend_until
                .map(|current| current.max(requested_deadline))
                .unwrap_or(requested_deadline),
        );
        self.animation.last_image_tick = now;
    }

    #[cfg(feature = "image")]
    pub(crate) fn refresh_image_layout_suspension(&mut self) {
        if self.surface.is_inline() {
            self.animation.last_image_layout_hash = None;
            self.animation.image_animation_suspend_until = None;
            self.animation.last_image_tick = self.core.ctx.env().now();
            return;
        }

        let new_hash = self.animated_image_layout_hash();
        if self.animation.last_image_layout_hash != new_hash {
            self.animation.last_image_layout_hash = new_hash;
            if new_hash.is_some() {
                let pause = Duration::from_millis(super::image_layout_stabilize_ms() as u64);
                self.suspend_image_animations_for(pause);
                image_support::suspend_image_rendering_for(pause);
            } else {
                self.animation.image_animation_suspend_until = None;
                self.animation.last_image_tick = self.core.ctx.env().now();
            }
        }
    }

    #[cfg(feature = "image")]
    fn animated_image_layout_hash(&self) -> Option<u64> {
        let mut hasher = DefaultHasher::new();
        let mut found = false;

        for &id in self.core.tree.animated_image_ids() {
            let node = self.core.tree.node(id);
            let NodeKind::Image(image) = &node.kind else {
                continue;
            };
            found = true;
            image.source_hash.hash(&mut hasher);
            node.rect.hash(&mut hasher);
        }

        found.then_some(hasher.finish())
    }

    #[cfg(feature = "image")]
    fn image_animation_viewport(&self) -> Option<Rect> {
        if !self.core.tree.is_valid(self.core.tree.root) {
            return None;
        }
        Some(self.core.tree.node(self.core.tree.root).rect)
    }
}
