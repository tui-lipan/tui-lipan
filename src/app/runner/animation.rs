#[cfg(feature = "image")]
use std::collections::hash_map::DefaultHasher;
#[cfg(feature = "image")]
use std::hash::{Hash, Hasher};
#[cfg(any(test, feature = "image"))]
use std::time::Duration;
#[cfg(feature = "image")]
use web_time::Instant;

#[cfg(feature = "image")]
use crate::backend::ratatui_backend::image_support;
use crate::core::component::Component;
use crate::core::node::NodeKind;
#[cfg(feature = "image")]
use crate::style::Rect;

use super::{AppRunner, spinner_frame_for_speed};

impl<C: Component> AppRunner<C> {
    pub(crate) fn update_spinner_frames(&mut self) {
        let spinner_ids = self.core.tree.spinner_ids().to_vec();

        for id in spinner_ids {
            match &mut self.core.tree.node_mut(id).kind {
                NodeKind::Spinner(node) if node.auto_frame => {
                    node.frame = spinner_frame_for_speed(self.animation.spinner_frame, node.speed);
                }
                NodeKind::DraggableTabBar(node) => {
                    for tab in std::sync::Arc::make_mut(&mut node.tabs) {
                        if let Some(spinner) = tab
                            .leading
                            .as_mut()
                            .and_then(|content| content.spinner_mut())
                            && spinner.auto_frame
                        {
                            spinner.spinner.frame = Some(spinner_frame_for_speed(
                                self.animation.spinner_frame,
                                spinner.spinner.speed,
                            ));
                        }
                    }
                }
                NodeKind::List(node) => {
                    for item in std::sync::Arc::make_mut(&mut node.items) {
                        if let Some(spinner) = item.status.as_mut().and_then(|s| s.spinner_mut())
                            && spinner.auto_frame
                        {
                            spinner.frame = spinner_frame_for_speed(
                                self.animation.spinner_frame,
                                spinner.speed,
                            );
                        }
                        if let Some(spinner) = item.gutter.as_mut().and_then(|g| g.spinner_mut())
                            && spinner.auto_frame
                        {
                            spinner.frame = spinner_frame_for_speed(
                                self.animation.spinner_frame,
                                spinner.speed,
                            );
                        }
                    }
                }
                _ => {}
            }
        }
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
            .is_some_and(|deadline| Instant::now() < deadline)
    }

    #[cfg(feature = "image")]
    pub(crate) fn suspend_image_animations_for(&mut self, duration: Duration) {
        let now = Instant::now();
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
            self.animation.last_image_tick = Instant::now();
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
                self.animation.last_image_tick = Instant::now();
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
