//! Shared runtime environment passed to every component.

use rustc_hash::FxHashMap;
use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;

use smallvec::SmallVec;

use crate::animation::AnimationRegistry;
use crate::app::context::SurfaceMode;
use crate::app::input::command_registry::CommandRegistry;
use crate::callback::ScopeId;
use crate::clipboard::{ClipboardConfig, ClipboardService};
use crate::core::component::{FocusContext, HoverContext, ScrollContext};
use crate::core::element::Element;
use crate::core::element::Key;
use crate::runtime::FocusRequest;
use crate::style::{HostTerminalColors, Rect, RichText, Theme};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DevToolsRequest {
    Show,
    Hide,
    Toggle,
}

#[derive(Clone)]
pub(crate) enum TranscriptEntry {
    Lines(Vec<RichText>),
    Element(Box<Element>),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MemoDependencies {
    pub(crate) theme: bool,
    pub(crate) focus: bool,
    pub(crate) hover: bool,
    pub(crate) scroll: SmallVec<[ScrollDependency; 2]>,
    pub(crate) mouse_capture: bool,
    pub(crate) viewport: bool,
    pub(crate) transition: bool,
    pub(crate) host_terminal_colors: bool,
    pub(crate) contexts: SmallVec<[(TypeId, &'static str); 2]>,
}

impl MemoDependencies {
    fn note(&mut self, dependency: MemoDependency) {
        match dependency {
            MemoDependency::Theme => self.theme = true,
            MemoDependency::Context { type_id, name } => {
                if !self.contexts.iter().any(|(id, _)| *id == type_id) {
                    self.contexts.push((type_id, name));
                }
            }
            MemoDependency::Focus => self.focus = true,
            MemoDependency::Hover => self.hover = true,
            MemoDependency::Scroll(dependency) => {
                if !self.scroll.contains(&dependency) {
                    self.scroll.push(dependency);
                }
            }
            MemoDependency::MouseCapture => self.mouse_capture = true,
            MemoDependency::Viewport => self.viewport = true,
            MemoDependency::Transition => self.transition = true,
            MemoDependency::HostTerminalColors => self.host_terminal_colors = true,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MemoDependencySnapshot {
    pub(crate) dependencies: MemoDependencies,
    pub(crate) theme_generation: u64,
    pub(crate) focus_generation: u64,
    pub(crate) hover_generation: u64,
    pub(crate) scroll_generations: SmallVec<[(ScrollDependency, u64); 2]>,
    pub(crate) mouse_capture_generation: u64,
    pub(crate) transition_generation: u64,
    pub(crate) host_terminal_color_generation: u64,
    pub(crate) viewport: Rect,
    pub(crate) context_generations: SmallVec<[(TypeId, &'static str, u64); 2]>,
}

impl MemoDependencySnapshot {
    pub(crate) fn matches(&self, env: &RuntimeEnv, viewport: Rect) -> bool {
        let context_generations = env.context_generations.borrow();
        (!self.dependencies.theme || self.theme_generation == env.active_theme_generation.get())
            && (!self.dependencies.focus || self.focus_generation == env.focus.generation())
            && (!self.dependencies.hover || self.hover_generation == env.hover.generation())
            && self
                .scroll_generations
                .iter()
                .all(|(dependency, generation)| {
                    env.scroll.dependency_generation(dependency) == *generation
                })
            && (!self.dependencies.mouse_capture
                || self.mouse_capture_generation == env.mouse_capture_generation.get())
            && (!self.dependencies.viewport || self.viewport == viewport)
            && (!self.dependencies.transition
                || self.transition_generation == env.animations.generation())
            && (!self.dependencies.host_terminal_colors
                || self.host_terminal_color_generation == env.host_terminal_color_generation.get())
            && self
                .context_generations
                .iter()
                .all(|(type_id, _name, generation)| {
                    context_generations.get(type_id).copied().unwrap_or(0) == *generation
                })
    }

    /// First dependency that fails the retain check (devtools diagnostics only).
    #[cfg(feature = "devtools")]
    pub(crate) fn first_mismatch(
        &self,
        env: &RuntimeEnv,
        viewport: Rect,
    ) -> Option<crate::core::nested::MemoDependencyKind> {
        use crate::core::nested::MemoDependencyKind;

        if self.dependencies.theme && self.theme_generation != env.active_theme_generation.get() {
            return Some(MemoDependencyKind::Theme);
        }
        if self.dependencies.focus && self.focus_generation != env.focus.generation() {
            return Some(MemoDependencyKind::Focus);
        }
        if self.dependencies.hover && self.hover_generation != env.hover.generation() {
            return Some(MemoDependencyKind::Hover);
        }
        if self
            .scroll_generations
            .iter()
            .any(|(dependency, generation)| {
                env.scroll.dependency_generation(dependency) != *generation
            })
        {
            return Some(MemoDependencyKind::Scroll);
        }
        if self.dependencies.mouse_capture
            && self.mouse_capture_generation != env.mouse_capture_generation.get()
        {
            return Some(MemoDependencyKind::MouseCapture);
        }
        if self.dependencies.viewport && self.viewport != viewport {
            return Some(MemoDependencyKind::Viewport);
        }
        if self.dependencies.transition && self.transition_generation != env.animations.generation()
        {
            return Some(MemoDependencyKind::Transition);
        }
        if self.dependencies.host_terminal_colors
            && self.host_terminal_color_generation != env.host_terminal_color_generation.get()
        {
            return Some(MemoDependencyKind::HostTerminalColors);
        }
        let context_generations = env.context_generations.borrow();
        for (type_id, name, generation) in &self.context_generations {
            if context_generations.get(type_id).copied().unwrap_or(0) != *generation {
                return Some(MemoDependencyKind::Context(*name));
            }
        }
        None
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MemoDependency {
    Theme,
    Context { type_id: TypeId, name: &'static str },
    Focus,
    Hover,
    Scroll(ScrollDependency),
    MouseCapture,
    Viewport,
    Transition,
    HostTerminalColors,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ScrollIdentity {
    pub(crate) scope: ScopeId,
    pub(crate) key: Key,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ScrollDependencyKind {
    Metrics,
    Scrollbars,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ScrollDependency {
    pub(crate) identity: ScrollIdentity,
    pub(crate) kind: ScrollDependencyKind,
}

/// Bundle of shared runtime handles cloned into every component context.
///
/// All `Rc`-wrapped fields are cheap to clone; `inline_mode` is `Copy`.
#[derive(Clone)]
pub(crate) struct RuntimeEnv {
    pub command_registry: CommandRegistry,
    pub quit: Rc<Cell<bool>>,
    pub focus: Rc<FocusContext>,
    pub hover: Rc<HoverContext>,
    pub scroll: Rc<ScrollContext>,
    pub animations: Rc<AnimationRegistry>,
    pub overlay_manager: Rc<RefCell<crate::overlay::OverlayManager>>,
    pub focus_request: Rc<RefCell<Option<FocusRequest>>>,
    pub mouse_capture: Rc<Cell<bool>>,
    pub surface_mode: SurfaceMode,
    pub transcript_history: Rc<RefCell<Vec<TranscriptEntry>>>,
    pub pending_transcript_entries: Rc<RefCell<VecDeque<TranscriptEntry>>>,
    pub clipboard: Rc<ClipboardService>,
    pub clipboard_config: ClipboardConfig,
    pub active_theme: Rc<RefCell<Theme>>,
    pub active_theme_generation: Rc<Cell<u64>>,
    pub effect_phase: Rc<Cell<u64>>,
    pub contexts: Rc<RefCell<FxHashMap<TypeId, Arc<dyn Any>>>>,
    pub context_generations: Rc<RefCell<FxHashMap<TypeId, u64>>>,
    pub host_terminal_colors: Rc<Cell<Option<HostTerminalColors>>>,
    pub host_terminal_color_generation: Rc<Cell<u64>>,
    pub host_terminal_color_refresh_requested: Rc<Cell<bool>>,
    pub host_terminal_color_refresh_enabled: bool,
    pub mouse_capture_generation: Rc<Cell<u64>>,
    pub memo_dependency_recorder: Rc<RefCell<Option<MemoDependencies>>>,
    /// When set, the next frame performs a full reconcile and draw (e.g. after an external
    /// program repainted the host terminal).
    pub full_repaint: Rc<Cell<bool>>,
    /// Pending request to change devtools visibility on the UI thread.
    pub devtools_request: Rc<RefCell<Option<DevToolsRequest>>>,
    /// Pending UI snapshot export/delivery after the next render.
    pub ui_snapshot_request: Rc<RefCell<Option<crate::ui_snapshot::UiSnapshotRequest>>>,
    pub command_chord_pending: Rc<std::cell::Cell<bool>>,
}

impl RuntimeEnv {
    pub(crate) fn set_effect_phase(&self, phase: u64) {
        self.effect_phase.set(phase);
    }

    pub(crate) fn note_memo_dependency(&self, dependency: MemoDependency) {
        if let Some(recorder) = self.memo_dependency_recorder.borrow_mut().as_mut() {
            recorder.note(dependency);
        }
    }

    pub(crate) fn begin_memo_dependency_capture(&self) {
        *self.memo_dependency_recorder.borrow_mut() = Some(MemoDependencies::default());
    }

    pub(crate) fn finish_memo_dependency_capture(&self, viewport: Rect) -> MemoDependencySnapshot {
        let dependencies = self
            .memo_dependency_recorder
            .borrow_mut()
            .take()
            .unwrap_or_default();
        let context_generations_map = self.context_generations.borrow();
        let mut context_generations = SmallVec::new();
        for &(type_id, name) in &dependencies.contexts {
            context_generations.push((
                type_id,
                name,
                context_generations_map.get(&type_id).copied().unwrap_or(0),
            ));
        }
        let scroll_generations = dependencies
            .scroll
            .iter()
            .cloned()
            .map(|dependency| {
                let generation = self.scroll.dependency_generation(&dependency);
                (dependency, generation)
            })
            .collect();

        MemoDependencySnapshot {
            dependencies,
            theme_generation: self.active_theme_generation.get(),
            focus_generation: self.focus.generation(),
            hover_generation: self.hover.generation(),
            scroll_generations,
            mouse_capture_generation: self.mouse_capture_generation.get(),
            transition_generation: self.animations.generation(),
            host_terminal_color_generation: self.host_terminal_color_generation.get(),
            viewport,
            context_generations,
        }
    }

    pub(crate) fn host_terminal_colors(&self) -> Option<HostTerminalColors> {
        self.note_memo_dependency(MemoDependency::HostTerminalColors);
        self.host_terminal_colors.get()
    }

    pub(crate) fn host_terminal_color_generation(&self) -> u64 {
        self.note_memo_dependency(MemoDependency::HostTerminalColors);
        self.host_terminal_color_generation.get()
    }

    pub(crate) fn request_host_terminal_color_refresh(&self) {
        if self.host_terminal_color_refresh_enabled {
            self.host_terminal_color_refresh_requested.set(true);
        }
    }

    pub(crate) fn take_host_terminal_color_refresh_request(&self) -> bool {
        self.host_terminal_color_refresh_requested.replace(false)
    }

    pub(crate) fn set_host_terminal_colors(&self, colors: Option<HostTerminalColors>) -> bool {
        if self.host_terminal_colors.get() == colors {
            return false;
        }

        self.host_terminal_colors.set(colors);
        self.advance_host_terminal_color_generation();
        true
    }

    fn advance_host_terminal_color_generation(&self) {
        self.host_terminal_color_generation.set(
            self.host_terminal_color_generation
                .get()
                .wrapping_add(1)
                .max(1),
        );
    }
}
