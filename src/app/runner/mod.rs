use rustc_hash::{FxHashMap, FxHashSet};
use std::cell::Cell;
#[cfg(feature = "devtools")]
use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::Write;
use std::ops::Range;
use std::rc::Rc;
#[cfg(feature = "devtools")]
use std::sync::Mutex;
#[cfg(feature = "image")]
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;
#[cfg(feature = "devtools")]
use std::time::SystemTime;
use web_time::Instant;

use crate::Result;
use crate::backend::ratatui_backend::TerminalGuard;
#[cfg(feature = "image")]
use crate::backend::ratatui_backend::image_support;
use crate::backend::ratatui_backend::render::JoinIndex;
#[cfg(feature = "image")]
use crate::backend::ratatui_backend::renderers::image::image_protocol_ready_epoch;
use crate::backend::ratatui_backend::terminal_handoff::{
    drain_terminal_query_responses_preserving_input, pause_stdin_reader_for_terminal_query_with,
    stdin_reader_is_paused, take_handoff_full_repaint_request,
};
use crate::callback::{Callback, ScopeId};
#[cfg(not(feature = "clipboard"))]
use crate::clipboard::NoOpClipboardProvider;
#[cfg(feature = "clipboard")]
use crate::clipboard::SystemClipboardProvider;
use crate::clipboard::{
    ClipboardConfig, ClipboardProvider, ClipboardService, PasteShiftInsertBehavior,
};
use crate::core::component::{Component, Context};
use crate::core::element::Element;
use crate::core::event::{KeyCode, MouseEvent, MouseKind};
use crate::core::node::NodeId;
use crate::runtime::{RuntimeCore, RuntimeCoreConfig};
use crate::style::{HostTerminalColors, Rect, Theme, query_host_colors};
use crate::widgets::SpinnerSpeed;
use crossterm::event::Event as CEvent;
use crossterm::{cursor::MoveTo, execute, style::Print};
use ratatui::Terminal as RatatuiTerminal;

#[cfg(feature = "devtools")]
use crate::app::context::DevToolsConfig;
use crate::app::context::{App, ContrastPolicy, SurfaceMode, TextAreaNewlineBinding};
use crate::app::input::command_registry::CommandEntry;
use crate::app::input::convert::{to_key_event, to_mouse_event};
use crate::app::input::focus;
use crate::app::input::keyboard;
use crate::app::input::keymap::{Action, Keymap, KeymapConfig, KeymapRuntime};
use crate::app::input::runtime_dispatch::{
    FrameworkSideEffect, RuntimeKeyDispatchConfig, RuntimeKeyDispatchState,
};

mod animation;
mod animation_ticker;
mod drag;
pub(crate) mod events;
mod exit_view;
mod focus_events;
#[cfg(unix)]
pub(crate) mod input_coordinator;
mod key_dispatch;
mod messages;
mod mouse_clicks;
mod overlay;
mod render_service;
mod scroll_optimize;
mod surface_driver;
mod terminal;
mod terminal_service;

pub(crate) use crate::app::interaction_state::{
    ActiveDrag, AnimationState, DirtyLevel, DirtyTracker, DragState, FocusState,
    MouseTrackingState, ViewportMetrics, WidgetState,
};
use surface_driver::SurfaceDriver;
pub(crate) use terminal::TerminalManager;

#[derive(Clone, Debug, PartialEq, Eq)]
enum RunnerEvent {
    Terminal(CEvent),
    HostTerminalColors(HostTerminalColors),
    InputError(String),
}

type ExitViewFn<C> = dyn Fn(&C, &Context<C>) -> Element;

const HOST_COLOR_REFRESH_QUIET_WINDOW: Duration = Duration::from_millis(50);

#[allow(deprecated)]
fn invalidate_previous_frame<B: ratatui::backend::Backend>(terminal: &mut RatatuiTerminal<B>) {
    terminal.swap_buffers();
    for cell in &mut terminal.current_buffer_mut().content {
        // `skip` participates in equality but suppresses output only on the
        // next/current buffer. Setting it on the previous buffer therefore
        // guarantees every drawable cell in the next frame differs.
        cell.skip = true;
    }
    terminal.swap_buffers();
}

pub(super) fn spinner_frame_for_speed(frame: usize, speed: SpinnerSpeed) -> usize {
    const TICK_MS: usize = 50;

    match speed {
        SpinnerSpeed::Fast => frame,
        SpinnerSpeed::Normal => frame / 2,
        SpinnerSpeed::Slow => frame / 4,
        SpinnerSpeed::Custom { frame_ms } => {
            let frame_ms = usize::from(frame_ms).max(1);
            frame.saturating_mul(TICK_MS) / frame_ms
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FrameworkCommandAction {
    Quit,
    ToggleDevtools,
    FocusNext,
    FocusPrev,
    DismissOverlay,
}

fn framework_command_handler(
    queue: Rc<std::cell::RefCell<Vec<FrameworkCommandAction>>>,
    action: FrameworkCommandAction,
) -> Callback<()> {
    Callback::new(move |_| {
        queue.borrow_mut().push(action);
    })
}

#[derive(Clone, Debug)]
pub(crate) struct ScrollFrameSnapshot {
    pub(crate) node_id: NodeId,
    pub(crate) scroll_offset: u16,
    pub(crate) content_height: u16,
    pub(crate) content_hash: Option<u64>,
    pub(crate) viewport_height: u16,
    pub(crate) scroll_rows: Range<u16>,
    pub(crate) scrollbar_rect: Option<Rect>,
    pub(crate) show_scroll_indicators: bool,
}

fn active_drag_dirty_level(drag: &ActiveDrag) -> Option<DirtyLevel> {
    match drag {
        ActiveDrag::Scrollbar(_) | ActiveDrag::Splitter(_) => Some(DirtyLevel::LayoutOnly),
        ActiveDrag::DragDrop(_) => Some(DirtyLevel::PaintOnly),
        ActiveDrag::TextArea(_)
        | ActiveDrag::DocumentView(_)
        | ActiveDrag::Input(_)
        | ActiveDrag::HexArea(_) => Some(DirtyLevel::PaintOnly),
        #[cfg(feature = "terminal")]
        ActiveDrag::Terminal(_) => Some(DirtyLevel::PaintOnly),
        _ => None,
    }
}

fn effective_active_drag_dirty_level(drag: &DragState) -> Option<DirtyLevel> {
    let base = active_drag_dirty_level(&drag.active);
    if drag.autoscroll_layout_dirty {
        Some(DirtyLevel::LayoutOnly)
    } else {
        base
    }
}

fn mouse_dispatch_dirty_level(
    kind: MouseKind,
    before: Option<DirtyLevel>,
    after: Option<DirtyLevel>,
) -> DirtyLevel {
    if matches!(kind, MouseKind::Moved) {
        return DirtyLevel::PaintOnly;
    }

    after
        .or({
            if matches!(kind, MouseKind::Up(_)) {
                before
            } else {
                None
            }
        })
        // Mouse-down can change focus, and component views/layout policies may
        // depend on focus (for example accordion panels and hint bars).
        .unwrap_or(if matches!(kind, MouseKind::Down(_)) {
            DirtyLevel::Full
        } else {
            DirtyLevel::PaintOnly
        })
}

fn apply_dirty_level(dirty: &mut DirtyTracker, level: DirtyLevel) {
    match level {
        DirtyLevel::None => {}
        DirtyLevel::PaintOnly => dirty.mark_paint(),
        DirtyLevel::LayoutOnly => dirty.mark_layout(),
        DirtyLevel::Full => dirty.mark_full(),
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct KeyDispatchResult {
    handled: bool,
    /// Dirty level requested by a specific handler. `Some(DirtyLevel::None)` is
    /// meaningful: the key was handled but no render is needed (for example a
    /// copy with feedback disabled or a clipboard no-op).
    dirty_override: Option<DirtyLevel>,
}

fn preserve_pending_event(pending_event: &mut Option<RunnerEvent>, event: RunnerEvent) {
    if pending_event.is_none() {
        *pending_event = Some(event);
    }
}

fn deferred_host_color_refresh_deadline(now: Instant) -> Instant {
    now + HOST_COLOR_REFRESH_QUIET_WINDOW
}

fn host_color_refresh_wait_remaining(
    quiet_until: Option<Instant>,
    now: Instant,
) -> Option<Duration> {
    quiet_until
        .filter(|deadline| now < *deadline)
        .map(|deadline| deadline.saturating_duration_since(now))
}

#[cfg(unix)]
fn uses_termina_live_input(surface: &SurfaceDriver, host_color_refresh_enabled: bool) -> bool {
    !surface.is_inline() && host_color_refresh_enabled
}

struct PlatformInputCoordinator {
    #[cfg(unix)]
    termina: Option<input_coordinator::TerminaInputCoordinator>,
}

impl PlatformInputCoordinator {
    fn start(
        surface: &SurfaceDriver,
        host_color_refresh_enabled: bool,
        initial_colors: Option<HostTerminalColors>,
        panic_input_control: crate::backend::ratatui_backend::terminal_handoff::InputHandoffSlot,
    ) -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            let termina = uses_termina_live_input(surface, host_color_refresh_enabled)
                .then(|| {
                    input_coordinator::TerminaInputCoordinator::start(
                        initial_colors,
                        panic_input_control,
                    )
                })
                .transpose()?;
            Ok(Self { termina })
        }
        #[cfg(not(unix))]
        {
            let _ = (
                surface,
                host_color_refresh_enabled,
                initial_colors,
                panic_input_control,
            );
            Ok(Self {})
        }
    }

    fn owns_fullscreen_input(&self) -> bool {
        #[cfg(unix)]
        return self.termina.is_some();
        #[cfg(not(unix))]
        return false;
    }

    fn receiver<'a>(
        &'a self,
        fallback: Option<&'a mpsc::Receiver<RunnerEvent>>,
    ) -> Option<&'a mpsc::Receiver<RunnerEvent>> {
        #[cfg(unix)]
        if let Some(coordinator) = &self.termina {
            return Some(coordinator.receiver());
        }
        fallback
    }

    fn route_host_color_refresh(&self, requested: bool) -> bool {
        #[cfg(unix)]
        if let Some(coordinator) = &self.termina {
            if requested {
                coordinator.request_host_color_refresh();
            }
            return false;
        }
        requested
    }
}

/// A mounted and runnable app.
pub struct AppRunner<C: Component> {
    pub(crate) title: Option<String>,
    pub(crate) surface: SurfaceDriver,
    pub(crate) core: RuntimeCore<C>,
    pub(crate) focus: FocusState,
    on_focus_changed: Option<crate::app::context::FocusChangedHook>,
    pub(crate) drag: DragState,
    pub(crate) mouse: MouseTrackingState,
    pub(crate) animation: AnimationState,
    pub(crate) copy_feedback: crate::app::copy_feedback::CopyFeedbackState,
    pub(crate) widgets: WidgetState,
    pub(crate) terminal: TerminalManager,
    pub(crate) clipboard: Rc<ClipboardService>,
    pub(crate) clipboard_config: ClipboardConfig,
    pub(crate) keymap: Keymap,
    pub(crate) keymap_runtime: KeymapRuntime,
    key_dispatch_config: RuntimeKeyDispatchConfig,
    key_dispatch_state: RuntimeKeyDispatchState,
    framework_effects: Vec<FrameworkSideEffect>,
    framework_command_queue: Rc<std::cell::RefCell<Vec<FrameworkCommandAction>>>,
    pub(crate) text_area_newline_binding: TextAreaNewlineBinding,
    pub(crate) contrast_policy: ContrastPolicy,
    pub(crate) mouse_enabled: bool,
    pub(crate) scroll_wheel_multiplier: u16,
    /// Genuine input events recovered from the stdin queue after a host-color OSC
    /// probe (e.g. wheel scrolls performed right after a `FocusGained`). Re-fed
    /// into the event loop ahead of the channel so the probe's blocking round-trip
    /// no longer eats them. See [`drain_terminal_query_responses_preserving_input`].
    pub(crate) pending_reinjected_input: VecDeque<CEvent>,
    pub(crate) mouse_capture_requested: Rc<Cell<bool>>,
    pub(crate) mouse_capture_active: bool,
    pub(crate) mouse_all_motion_enabled: bool,
    /// Persistent JoinIndex for frame-adjacency lookups - rebuilt after
    /// reconciliation, reused for paint-only frames.
    pub(crate) cached_join_index: JoinIndex,
    /// Persistent cache for scrollbar metrics - survives across paint-only
    /// frames so that scrollbar thumb positions are not recomputed when only
    /// cosmetic properties (blink, spinner) change.
    pub(crate) scrollbar_metrics_cache:
        std::cell::RefCell<crate::utils::scrollbar::ScrollbarMetricsCache>,
    /// Per-draw Spinner/ProgressBar glyph memo slab (borrowed via `RenderContext`).
    pub(crate) paint_glyph_caches: std::rc::Rc<
        std::cell::RefCell<crate::backend::ratatui_backend::glyph_paint_cache::PaintGlyphCaches>,
    >,
    /// Reusable overlay cell snapshot buffer for transparent overlays.
    pub(crate) overlay_bg_snapshot: std::cell::RefCell<Vec<ratatui::buffer::Cell>>,
    /// Cached terminal cells for `DragPreview::SourceSnapshot` after the source subtree is collapsed.
    pub(crate) dnd_snapshot_cells:
        std::cell::RefCell<Option<(u16, u16, Vec<ratatui::buffer::Cell>)>>,
    /// Copy of the last presented terminal buffer for scroll fast paths.
    pub(crate) last_frame_snapshot: Option<ratatui::buffer::Buffer>,
    /// Scratch buffer used to build the pre-scroll diff baseline.
    pub(crate) scroll_diff_snapshot: Option<ratatui::buffer::Buffer>,
    /// Reusable scratch terminal for inline transcript element commits.
    pub(crate) inline_commit_scratch:
        Option<ratatui::Terminal<ratatui::backend::CrosstermBackend<Vec<u8>>>>,
    /// ScrollView geometry captured from the last presented frame.
    pub(crate) last_scroll_frames: Vec<ScrollFrameSnapshot>,
    /// Tracks the last tree epoch for post-reconcile cache pruning.
    pub(crate) last_post_reconcile_epoch: u32,
    /// Nested component scopes whose cached subtree must be refreshed before
    /// the next layout-only reconcile. Deduplicated at insert time via `dirty_scope_set`.
    pub(crate) dirty_component_scopes: Vec<ScopeId>,
    /// Tracks which scopes are already in `dirty_component_scopes` for O(1) dedup.
    dirty_scope_set: FxHashSet<ScopeId>,
    /// Previous list `selected` index per node (for pointer-hover suppression).
    pub(crate) last_seen_list_selection: FxHashMap<NodeId, Option<usize>>,
    /// Previous table `selected` index per node.
    pub(crate) last_seen_table_selection: FxHashMap<NodeId, Option<usize>>,
    /// Resolved terminal background color for opacity blending through
    /// `Color::Reset` backgrounds.
    pub(crate) terminal_bg: Option<crate::style::Color>,
    /// Opt-in root viewport background painted before the UI tree each frame.
    pub(crate) screen_background: crate::app::ScreenBackground,
    pub(crate) system_theme: bool,
    #[cfg(feature = "devtools")]
    pub(crate) devtools_state: Rc<RefCell<crate::devtools::DevToolsState>>,
    #[cfg(feature = "devtools")]
    pub(crate) devtools_log_queue: Arc<Mutex<VecDeque<crate::debug::DevLogEntry>>>,
    #[cfg(feature = "devtools")]
    pub(crate) devtools_config: DevToolsConfig,
    /// Set after an app frame records new DevTools metrics. The panel's extra
    /// root is built *before* those metrics are recorded (install happens at the
    /// top of a render, recording at the bottom), so the just-drawn panel shows
    /// the previous frame. This requests exactly one idle "catch-up" frame that
    /// rebuilds the panel with the freshly recorded metrics. The catch-up frame
    /// records no metrics (see `devtools_metrics_suppressed`), so it cannot
    /// re-arm this flag and the refresh terminates after a single frame.
    #[cfg(feature = "devtools")]
    devtools_refresh_pending: bool,
    /// True only for the duration of a catch-up refresh frame. Suppresses metric
    /// recording so the refresh does not feed back into `devtools_refresh_pending`.
    #[cfg(feature = "devtools")]
    devtools_metrics_suppressed: bool,
    /// Pending update attributions coalesced until the next recorded metrics frame.
    #[cfg(feature = "devtools")]
    pending_attributions: Vec<crate::devtools::state::UpdateAttribution>,
    /// Lazy cache of the root component's short display name for attribution.
    #[cfg(feature = "devtools")]
    root_component_display_name: Option<Arc<str>>,
    pub(crate) exit_view_fn: Option<Box<ExitViewFn<C>>>,
    #[cfg(debug_assertions)]
    debug_paint_claim_root: bool,
}

impl<C: Component> AppRunner<C> {
    pub(crate) fn new(app: App, component: C, props: C::Properties) -> Self {
        #[cfg(feature = "devtools")]
        crate::debug::set_devtools_logs_enabled(app.devtools_config.logs);

        let surface = SurfaceDriver::new(app.surface_mode);
        let inline_mode = surface.is_inline();
        let mouse_enabled = app.mouse_enabled.unwrap_or(!inline_mode);
        let mouse_capture_requested = Rc::new(Cell::new(mouse_enabled));
        let host_terminal_color_refresh_enabled = app.live_host_terminal_colors || app.system_theme;

        let mut clipboard_config = app.clipboard_config;
        let system_provider: Box<dyn ClipboardProvider> =
            app.clipboard_provider.unwrap_or_else(|| {
                #[cfg(feature = "clipboard")]
                {
                    Box::new(SystemClipboardProvider::new())
                }
                #[cfg(not(feature = "clipboard"))]
                {
                    Box::new(NoOpClipboardProvider)
                }
            });
        if clipboard_config.enable_primary_selection
            && !system_provider.supports_primary_selection()
        {
            clipboard_config.enable_primary_selection = false;
        }
        if !clipboard_config.enable_primary_selection
            && matches!(
                clipboard_config.paste_shift_insert_behavior,
                PasteShiftInsertBehavior::PrimarySelection
            )
        {
            clipboard_config.paste_shift_insert_behavior = PasteShiftInsertBehavior::Clipboard;
        }

        let clipboard = Rc::new(ClipboardService::new(
            system_provider,
            app.clipboard_reporter,
        ));

        let core = RuntimeCore::new(
            component,
            props,
            RuntimeCoreConfig {
                viewport: Rect::default(),
                theme: app.theme.clone(),
                surface_mode: app.surface_mode,
                mouse_capture: mouse_capture_requested.clone(),
                clipboard: clipboard.clone(),
                clipboard_config: clipboard_config.clone(),
                host_terminal_color_refresh_enabled,
            },
        );
        core.overlay_manager
            .borrow_mut()
            .set_toast_placement(app.toast_placement);
        core.overlay_manager
            .borrow_mut()
            .set_toast_gap(app.toast_gap);
        core.overlay_manager
            .borrow_mut()
            .set_toast_margin(app.toast_margin);
        let mut keymap_config = KeymapConfig::from_clipboard_config(&clipboard_config);
        if let Some(path) = app.keymap_path.clone() {
            keymap_config = keymap_config.keymap_path(path);
        }
        keymap_config = keymap_config
            .framework_keymap(app.framework_keymap.clone())
            .user_keymap_policy(app.user_keymap_policy);
        let keymap = Keymap::new(keymap_config);
        let keymap_runtime = KeymapRuntime::new(&keymap);
        let key_dispatch_config = RuntimeKeyDispatchConfig {
            focus_policy: app.focus_policy,
            key_dispatch_policy: app.key_dispatch_policy,
            terminal_key_policy: app.terminal_key_policy,
            command_conflict_policy: app.command_conflict_policy,
            chord_mismatch_policy: app.chord_mismatch_policy,
        };
        let framework_command_queue = Rc::new(std::cell::RefCell::new(Vec::new()));

        let command_registry = core.ctx.command_registry();
        command_registry.register(
            CommandEntry::builder("app.quit")
                .label("Quit application")
                .description("Exit the current app")
                .category("Application")
                .keybinding_from_keymap(&keymap, Action::Quit)
                .handler(framework_command_handler(
                    framework_command_queue.clone(),
                    FrameworkCommandAction::Quit,
                ))
                .build(),
        );
        command_registry.register(
            CommandEntry::builder("app.toggle-devtools")
                .label("Toggle DevTools")
                .description("Show or hide the built-in DevTools panel")
                .category("Application")
                .keybinding_from_keymap(&keymap, Action::ToggleDevTools)
                .handler(framework_command_handler(
                    framework_command_queue.clone(),
                    FrameworkCommandAction::ToggleDevtools,
                ))
                .build(),
        );
        command_registry.register(
            CommandEntry::builder("app.focus-next")
                .label("Focus next")
                .description("Move focus to the next focusable widget")
                .category("Application")
                .keybinding_from_keymap(&keymap, Action::FocusNext)
                .handler(framework_command_handler(
                    framework_command_queue.clone(),
                    FrameworkCommandAction::FocusNext,
                ))
                .build(),
        );
        command_registry.register(
            CommandEntry::builder("app.focus-prev")
                .label("Focus previous")
                .description("Move focus to the previous focusable widget")
                .category("Application")
                .keybinding_from_keymap(&keymap, Action::FocusPrev)
                .handler(framework_command_handler(
                    framework_command_queue.clone(),
                    FrameworkCommandAction::FocusPrev,
                ))
                .build(),
        );
        command_registry.register(
            CommandEntry::builder("app.dismiss-overlay")
                .label("Dismiss overlay")
                .description("Close the top-most dismissible overlay")
                .category("Application")
                .keybinding_from_keymap(&keymap, Action::DismissOverlay)
                .handler(framework_command_handler(
                    framework_command_queue.clone(),
                    FrameworkCommandAction::DismissOverlay,
                ))
                .build(),
        );
        let key_dispatch_state =
            RuntimeKeyDispatchState::new(&command_registry, app.command_conflict_policy);

        #[cfg(feature = "image")]
        let animation = AnimationState {
            last_image_protocol_epoch: image_protocol_ready_epoch(),
            ..AnimationState::default()
        };
        #[cfg(not(feature = "image"))]
        let animation = AnimationState::default();

        #[cfg(feature = "devtools")]
        let devtools_log_queue = Arc::new(Mutex::new(VecDeque::new()));

        let focus = FocusState {
            policy: app.focus_policy,
            ..FocusState::default()
        };
        let on_focus_changed = app.on_focus_changed.clone();

        AppRunner {
            title: app.title,
            surface,
            core,
            focus,
            on_focus_changed,
            drag: DragState::default(),
            mouse: MouseTrackingState::default(),
            animation,
            copy_feedback: Default::default(),
            widgets: WidgetState::default(),
            terminal: TerminalManager::default(),
            clipboard,
            clipboard_config,
            keymap,
            keymap_runtime,
            key_dispatch_config,
            key_dispatch_state,
            framework_effects: Vec::new(),
            framework_command_queue,
            text_area_newline_binding: app.text_area_newline_binding,
            contrast_policy: app.contrast_policy,
            mouse_enabled,
            scroll_wheel_multiplier: app.scroll_wheel_multiplier.max(1),
            pending_reinjected_input: VecDeque::new(),
            mouse_capture_requested,
            mouse_capture_active: mouse_enabled,
            mouse_all_motion_enabled: false,
            cached_join_index: JoinIndex::default(),
            scrollbar_metrics_cache: std::cell::RefCell::new(Default::default()),
            paint_glyph_caches: std::rc::Rc::new(std::cell::RefCell::new(Default::default())),
            overlay_bg_snapshot: std::cell::RefCell::new(Vec::new()),
            dnd_snapshot_cells: std::cell::RefCell::new(None),
            last_frame_snapshot: None,
            scroll_diff_snapshot: None,
            inline_commit_scratch: None,
            last_scroll_frames: Vec::new(),
            last_post_reconcile_epoch: 0,
            dirty_component_scopes: Vec::new(),
            dirty_scope_set: FxHashSet::default(),
            last_seen_list_selection: FxHashMap::default(),
            last_seen_table_selection: FxHashMap::default(),
            terminal_bg: app.terminal_bg,
            screen_background: app.screen_background,
            system_theme: app.system_theme,
            #[cfg(feature = "devtools")]
            devtools_state: Rc::new(RefCell::new({
                let mut state = crate::devtools::DevToolsState::default();
                state.set_hide_framework_logs(!app.devtools_config.show_framework_logs);
                state
            })),
            #[cfg(feature = "devtools")]
            devtools_log_queue,
            #[cfg(feature = "devtools")]
            devtools_config: app.devtools_config,
            #[cfg(feature = "devtools")]
            devtools_refresh_pending: false,
            #[cfg(feature = "devtools")]
            devtools_metrics_suppressed: false,
            #[cfg(feature = "devtools")]
            pending_attributions: Vec::new(),
            #[cfg(feature = "devtools")]
            root_component_display_name: None,
            exit_view_fn: None,
            #[cfg(debug_assertions)]
            debug_paint_claim_root: false,
        }
    }

    /// Render a final one-shot view to stdout after the app exits.
    ///
    /// The callback runs before component unmount while state is still alive,
    /// and the returned `Element` is rendered after terminal teardown.
    pub fn exit_view(mut self, f: impl Fn(&C, &Context<C>) -> Element + 'static) -> Self {
        self.exit_view_fn = Some(Box::new(f));
        self
    }

    fn host_terminal_color_refresh_enabled(&self) -> bool {
        self.core.ctx.env().host_terminal_color_refresh_enabled
    }

    fn refresh_host_terminal_colors(
        &mut self,
        wait_for_reader: bool,
        request_repaint: bool,
    ) -> bool {
        if !self.host_terminal_color_refresh_enabled() {
            return false;
        }

        // Pause the reader for the OSC round-trip but do NOT blanket-flush the
        // input queue on drop: genuine input (notably wheel scrolls right after a
        // FocusGained) can queue during the blocking probe. Recover it below and
        // re-inject it into the event loop; only OSC response garbage is dropped.
        let _pause = pause_stdin_reader_for_terminal_query_with(wait_for_reader, false);
        let colors = query_host_colors();
        match drain_terminal_query_responses_preserving_input() {
            Ok(preserved) => self.pending_reinjected_input.extend(preserved),
            Err(err) => crate::debug::internal_log!(
                "[tui-lipan] host color refresh: preserve-drain failed (non-fatal): {}",
                err
            ),
        }

        let Some(colors) = colors else {
            return false;
        };

        self.apply_host_terminal_colors(colors, request_repaint)
    }

    fn apply_host_terminal_colors(
        &mut self,
        colors: HostTerminalColors,
        request_repaint: bool,
    ) -> bool {
        let colors_changed = self.core.ctx.env().set_host_terminal_colors(Some(colors));
        let terminal_bg_changed = self.terminal_bg != Some(colors.bg);
        let mut theme_changed = false;

        if terminal_bg_changed {
            self.terminal_bg = Some(colors.bg);
        }
        if self.system_theme {
            let theme = Theme::from_host_colors(colors);
            if self.core.theme != theme {
                self.core.theme = theme.clone();
                self.core.ctx.set_active_theme(theme);
                theme_changed = true;
            }
        }

        if !(colors_changed || terminal_bg_changed || theme_changed) {
            return false;
        }
        if request_repaint {
            self.core.ctx.request_full_repaint();
        }
        true
    }

    fn request_host_terminal_color_refresh_from_event(&self) {
        self.core.ctx.env().request_host_terminal_color_refresh();
    }

    fn take_host_terminal_color_refresh_request(&self) -> bool {
        self.core
            .ctx
            .env()
            .take_host_terminal_color_refresh_request()
    }

    #[cfg(test)]
    fn dispatch_focused_key(&mut self, key: crate::core::event::KeyEvent) -> KeyDispatchResult {
        let focused = self.focus.focused;
        let mut key_ctx = crate::app::input::handlers::KeyCtx {
            read_only_selection: Some(&self.widgets.read_only_selection),
            input_history: &mut self.widgets.input_history,
            textarea_history: &mut self.widgets.textarea_history,
            text_area_vim_state: &mut self.widgets.text_area_vim_state,
            hex_history: &mut self.widgets.hex_history,
            hex_pending_edit: &mut self.widgets.hex_pending_edit,
            keymap: &self.keymap,
            text_area_newline_binding: self.text_area_newline_binding,
            clipboard: &self.clipboard,
            clipboard_config: &self.clipboard_config,
            copy_feedback: &mut self.copy_feedback,
            dirty_override: None,
        };
        let handled = keyboard::dispatch_key(&mut self.core.tree, focused, key, &mut key_ctx);
        KeyDispatchResult {
            handled,
            dirty_override: key_ctx.dirty_override,
        }
    }

    fn dispatch_selection_clipboard_shortcut(
        &mut self,
        key: crate::core::event::KeyEvent,
    ) -> KeyDispatchResult {
        let mut key_ctx = crate::app::input::handlers::KeyCtx {
            read_only_selection: Some(&self.widgets.read_only_selection),
            input_history: &mut self.widgets.input_history,
            textarea_history: &mut self.widgets.textarea_history,
            text_area_vim_state: &mut self.widgets.text_area_vim_state,
            hex_history: &mut self.widgets.hex_history,
            hex_pending_edit: &mut self.widgets.hex_pending_edit,
            keymap: &self.keymap,
            text_area_newline_binding: self.text_area_newline_binding,
            clipboard: &self.clipboard,
            clipboard_config: &self.clipboard_config,
            copy_feedback: &mut self.copy_feedback,
            dirty_override: None,
        };
        let handled =
            keyboard::dispatch_selection_clipboard_shortcut(&mut self.core.tree, key, &mut key_ctx);
        KeyDispatchResult {
            handled,
            dirty_override: key_ctx.dirty_override,
        }
    }

    fn dispatch_focused_paste(&mut self, text: &str) -> bool {
        let focused = self.focus.focused;
        let mut key_ctx = crate::app::input::handlers::KeyCtx {
            read_only_selection: Some(&self.widgets.read_only_selection),
            input_history: &mut self.widgets.input_history,
            textarea_history: &mut self.widgets.textarea_history,
            text_area_vim_state: &mut self.widgets.text_area_vim_state,
            hex_history: &mut self.widgets.hex_history,
            hex_pending_edit: &mut self.widgets.hex_pending_edit,
            keymap: &self.keymap,
            text_area_newline_binding: self.text_area_newline_binding,
            clipboard: &self.clipboard,
            clipboard_config: &self.clipboard_config,
            copy_feedback: &mut self.copy_feedback,
            dirty_override: None,
        };
        keyboard::dispatch_paste(&mut self.core.tree, focused, text, &mut key_ctx)
    }

    #[cfg(feature = "devtools")]
    fn setup_devtools_log_sink(&mut self) {
        let queue = Arc::clone(&self.devtools_log_queue);
        crate::debug::set_devtools_log_sink(move |entry| {
            if let Ok(mut pending) = queue.lock() {
                pending.push_back(entry);
            }
        });
    }

    #[cfg(feature = "devtools")]
    fn ingest_pending_devtools_logs(&mut self) {
        let Ok(mut pending) = self.devtools_log_queue.try_lock() else {
            return;
        };

        if !self.devtools_config.logs {
            // Belt-and-suspenders: the sink is unregistered when logs is false,
            // so the queue should already be empty. Clear defends against the
            // atomic being flipped mid-run or tests pushing directly.
            pending.clear();
            return;
        }

        let visible = self.devtools_state.borrow().visible;

        while let Some(entry) = pending.pop_front() {
            let mut state = self.devtools_state.borrow_mut();
            let entry = crate::devtools::DevLogEntry {
                timestamp: SystemTime::now(),
                message: entry.message,
                source: entry.source,
            };
            if visible {
                state.push_log_entry(entry);
            } else {
                state.push_log_entry_hidden(entry);
            }
        }
    }

    #[cfg(feature = "devtools")]
    fn set_devtools_visible(&mut self, visible: bool) -> bool {
        let mut state = self.devtools_state.borrow_mut();
        let was_visible = state.visible;
        if was_visible == visible {
            return false;
        }

        state.set_visible(visible);
        if visible && !state.log_paused {
            state.sync_logs();
        }
        true
    }

    #[cfg(feature = "devtools")]
    fn note_attribution(
        &mut self,
        source: crate::devtools::state::UpdateSource,
        level: DirtyLevel,
    ) {
        if matches!(level, DirtyLevel::None) {
            return;
        }
        if !self.devtools_config.metrics {
            return;
        }
        if !self.devtools_state.borrow().visible {
            return;
        }
        if self.devtools_metrics_suppressed {
            return;
        }
        crate::devtools::state::note_update_attribution(
            &mut self.pending_attributions,
            source,
            level,
        );
    }

    #[cfg(feature = "devtools")]
    fn apply_input_dirty(
        &mut self,
        dirty: &mut DirtyTracker,
        level: DirtyLevel,
        label: &'static str,
    ) {
        apply_dirty_level(dirty, level);
        self.note_attribution(crate::devtools::state::UpdateSource::Input(label), level);
    }

    #[cfg(feature = "devtools")]
    fn apply_pending_devtools_request(&mut self) -> bool {
        match self.core.ctx.take_devtools_request() {
            Some(crate::core::runtime_env::DevToolsRequest::Show) => {
                self.set_devtools_visible(true)
            }
            Some(crate::core::runtime_env::DevToolsRequest::Hide) => {
                self.set_devtools_visible(false)
            }
            Some(crate::core::runtime_env::DevToolsRequest::Toggle) => {
                let visible = self.devtools_state.borrow().visible;
                self.set_devtools_visible(!visible)
            }
            None => false,
        }
    }

    #[cfg(not(feature = "devtools"))]
    fn apply_pending_devtools_request(&mut self) -> bool {
        self.core.ctx.take_devtools_request().is_some()
    }

    /// Resolve the opt-in root viewport background to a concrete fill style.
    ///
    /// `ScreenBackground::Theme` tracks the *active root theme* — the theme of
    /// the realized root node — so apps that swap themes via a root
    /// `ThemeProvider` (rather than re-building the `App`) keep the backdrop in
    /// sync. Falls back to the startup app theme before the first reconcile.
    pub(crate) fn resolved_screen_background(&self) -> Option<ratatui::style::Style> {
        let app_root = self.app_content_root_node();
        let theme = if self.core.tree.is_valid(app_root) {
            self.core.tree.node(app_root).active_theme()
        } else {
            &self.core.theme
        };
        self.screen_background
            .resolve(theme)
            .map(crate::backend::ratatui_backend::common::to_ratatui_style)
    }

    fn app_content_root_node(&self) -> crate::core::node::NodeId {
        let root = self.core.tree.root;
        if self.core.extra_root_element.is_some() && self.core.tree.is_valid(root) {
            let node = self.core.tree.node(root);
            if matches!(node.kind, crate::core::node::NodeKind::ZStack(_))
                && let Some(&base) = node.children.first()
            {
                return base;
            }
        }
        root
    }

    fn apply_pending_ui_snapshot_request(&mut self) -> crate::Result<()> {
        let Some(request) = self.core.ctx.take_ui_snapshot_request() else {
            return Ok(());
        };
        let screen_background = self.resolved_screen_background();
        let interaction = crate::backend::ratatui_backend::capture_render::CaptureInteraction {
            focused: self.focus.focused,
            hovered: self.mouse.hovered,
            mouse_pos: self.mouse.last_mouse,
        };
        let snapshot = crate::ui_snapshot::build_ui_snapshot(
            &self.core.tree,
            self.core.ctx.viewport(),
            interaction,
            self.core.ctx.env().effect_phase.get(),
            screen_background,
            &crate::ui_snapshot::UiSnapshotOptions::default(),
        );
        match request {
            crate::ui_snapshot::UiSnapshotRequest::Write { path, format } => {
                crate::ui_snapshot::write_snapshot(&snapshot, &path, format)?;
            }
            crate::ui_snapshot::UiSnapshotRequest::Deliver(slot) => {
                *slot.borrow_mut() = Some(snapshot);
            }
        }
        Ok(())
    }

    /// Run the application event loop.
    pub fn run(mut self) -> Result<()> {
        crate::debug::init_logging();
        #[cfg(feature = "devtools")]
        {
            // The atomic was already set from config in AppRunner::new.
            // We only register the sink here because it needs the queue
            // which is owned by the runner and not available earlier.
            if self.devtools_config.logs {
                self.setup_devtools_log_sink();
            }
        }
        let debug_enabled = crate::debug::enabled();
        let panic_surface_mode = self.surface.mode();
        let mut exit_element: Option<Element> = None;
        let contrast_policy = self.contrast_policy;
        let panic_keyboard_enhancement = Arc::new(AtomicBool::new(false));
        let panic_theme_notifications = Arc::new(AtomicBool::new(false));
        let panic_input_control =
            crate::backend::ratatui_backend::terminal_handoff::input_handoff_slot();
        let hook_keyboard_enhancement = Arc::clone(&panic_keyboard_enhancement);
        let hook_theme_notifications = Arc::clone(&panic_theme_notifications);
        let hook_input_control = Arc::clone(&panic_input_control);
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            crate::backend::ratatui_backend::terminal_handoff::pause_input_from_slot(
                &hook_input_control,
            );
            crate::backend::ratatui_backend::restore_terminal_on_panic(
                panic_surface_mode,
                hook_keyboard_enhancement.load(Ordering::SeqCst),
                hook_theme_notifications.load(Ordering::SeqCst),
            );
            if debug_enabled {
                crate::debug::internal_log!("[tui-lipan] panic: {}", info);
            }
            default_hook(info);
        }));

        let result = (|| -> Result<()> {
            let (mut terminal, guard) = TerminalGuard::enter(
                self.surface.mode(),
                self.mouse_enabled,
                panic_keyboard_enhancement.as_ref(),
            )?;
            let mut guard = guard;
            self.refresh_host_terminal_colors(false, false);

            // Fullscreen Unix apps that opted into live host colors use Termina
            // as their sole runtime decoder. Startup probing above deliberately
            // finishes before this worker takes ownership of terminal input.
            let platform_input = PlatformInputCoordinator::start(
                &self.surface,
                self.host_terminal_color_refresh_enabled(),
                self.core.ctx.host_terminal_colors(),
                Arc::clone(&panic_input_control),
            )?;
            if platform_input.owns_fullscreen_input() {
                let notifications_enabled = guard.enable_theme_notifications()?;
                panic_theme_notifications.store(notifications_enabled, Ordering::SeqCst);
            }

            // Root init runs once with the initial viewport.
            {
                let size = terminal.size()?;

                let bounds = self.content_bounds(size.width, size.height);
                self.core.ctx.set_viewport(bounds);
                self.core.init();

                if self.apply_pending_devtools_request() {
                    self.core.ctx.request_full_repaint();
                }

                // Perform an initial render pass to build the tree and resolve focus.
                self.push_drag_layout_collapse_hint();
                self.core.render_element(
                    bounds,
                    None,
                    self.focus.focused_key.as_ref(),
                    self.mouse.hovered,
                );
                self.pop_drag_layout_collapse_hint();
                // Honor a focus request issued during the initial expand
                // (root `init()` or any child component's `init()`) before
                // falling back to the first focusable node.
                self.apply_pending_focus_request();
                focus::restore_focus(
                    &self.core.tree,
                    &mut self.focus.focused,
                    &mut self.focus.focused_key,
                    &mut self.focus.focused_tag,
                    self.focus.policy,
                );
            }

            // Initial render.
            self.render(&mut terminal)?;

            // Inline mode must read events on the main thread because ratatui
            // inline autoresize queries cursor position from stdin. A concurrent
            // crossterm reader thread can consume cursor-report bytes and cause
            // viewport desync/timeouts during resize.
            let mut crossterm_event_rx: Option<mpsc::Receiver<RunnerEvent>> = None;
            if !self.surface.is_inline() && !platform_input.owns_fullscreen_input() {
                // Fullscreen path keeps the background reader for low-latency wakeups.
                let (event_tx, rx) = mpsc::channel::<RunnerEvent>();
                std::thread::Builder::new()
                    .name("crossterm-reader".into())
                    .spawn({
                        let tx = event_tx.clone();
                        move || {
                            loop {
                                while stdin_reader_is_paused() {
                                    std::thread::sleep(Duration::from_millis(25));
                                }
                                match crossterm::event::poll(Duration::from_millis(100)) {
                                    Ok(true) => match crossterm::event::read() {
                                        Ok(ev) => {
                                            if tx.send(RunnerEvent::Terminal(ev)).is_err() {
                                                break;
                                            }
                                        }
                                        Err(err) => {
                                            let _ =
                                                tx.send(RunnerEvent::InputError(err.to_string()));
                                            break;
                                        }
                                    },
                                    Ok(false) => {}
                                    Err(err) => {
                                        let _ = tx.send(RunnerEvent::InputError(err.to_string()));
                                        break;
                                    }
                                }
                            }
                        }
                    })?;
                // Drop our copy so the channel closes when the loop ends.
                drop(event_tx);
                crossterm_event_rx = Some(rx);
            }

            let event_rx = platform_input.receiver(crossterm_event_rx.as_ref());

            let mut pending_event: Option<RunnerEvent> = None;
            let mut host_color_refresh_quiet_until: Option<Instant> = None;
            let mut deferred_full = false;
            // Last terminal size we observed. Used as a fallback for missed
            // SIGWINCH/Resize events (see the size-poll check below).
            let mut last_known_size = terminal.size().unwrap_or_default();

            while !self.core.ctx.should_quit() {
                #[cfg(feature = "profiling-tracing")]
                let frame_start = Instant::now();

                let mut dirty = DirtyTracker::default();
                #[cfg(feature = "devtools")]
                self.ingest_pending_devtools_logs();
                if deferred_full {
                    dirty.mark_full();
                    deferred_full = false;
                }
                let mut poll_timeout = self.update_animation_cycle(&mut dirty);
                if let Some(remaining) = host_color_refresh_wait_remaining(
                    host_color_refresh_quiet_until,
                    Instant::now(),
                ) {
                    poll_timeout = poll_timeout.min(remaining);
                }

                // Fallback for missed Resize events. In fullscreen mode the
                // loop relies on crossterm delivering a Resize event (driven by
                // SIGWINCH) to redraw. Some compositors — e.g. a tiling WM
                // reflowing our window after a neighbor closes — resize the PTY
                // without that event reaching us, so the UI stays at the old
                // size until the next unrelated input. Poll the real size each
                // tick and force a full redraw when it drifts. Only relevant on
                // the background-reader (fullscreen) path; inline mode reads
                // events on the main thread and relies on ratatui autoresize.
                if event_rx.is_some()
                    && let Ok(size) = terminal.size()
                    && size != last_known_size
                {
                    last_known_size = size;
                    host_color_refresh_quiet_until =
                        Some(deferred_host_color_refresh_deadline(Instant::now()));
                    crate::debug::internal_log!("[tui-lipan] dirty: resize (size poll)");
                    dirty.mark_full();
                }

                if self.sync_mouse_capture_preference(&mut terminal)? {
                    dirty.mark_full();
                }

                let actual_timeout = if dirty.is_dirty()
                    || pending_event.is_some()
                    || !self.pending_reinjected_input.is_empty()
                {
                    Duration::from_millis(0)
                } else {
                    poll_timeout.max(Duration::from_millis(1))
                };

                let maybe_event = if let Some(ev) = pending_event.take() {
                    Some(ev)
                } else if let Some(ev) = self.pending_reinjected_input.pop_front() {
                    // Genuine input recovered from a prior host-color OSC probe,
                    // replayed ahead of the channel so it is not lost to the blocking
                    // round-trip that ran while it was queued.
                    Some(RunnerEvent::Terminal(ev))
                } else {
                    self.recv_event(actual_timeout, event_rx)?
                };
                let maybe_event = match maybe_event {
                    Some(RunnerEvent::HostTerminalColors(colors)) => {
                        self.apply_host_terminal_colors(colors, true);
                        None
                    }
                    Some(RunnerEvent::InputError(message)) => {
                        return Err(std::io::Error::other(message).into());
                    }
                    Some(RunnerEvent::Terminal(event)) => Some(event),
                    None => None,
                };
                if let Some(event) = maybe_event {
                    match event {
                        CEvent::FocusGained => {
                            self.focus.window_focused = true;
                            self.animation.reset_blink();
                            host_color_refresh_quiet_until =
                                Some(deferred_host_color_refresh_deadline(Instant::now()));
                            self.request_host_terminal_color_refresh_from_event();
                            dirty.mark_paint();
                        }
                        CEvent::FocusLost => {
                            self.focus.window_focused = false;
                            self.animation.reset_blink();
                            dirty.mark_paint();
                        }
                        CEvent::Key(k) => {
                            if let Some(key) = to_key_event(k) {
                                if matches!(key.code, KeyCode::Esc)
                                    && matches!(self.drag.active, ActiveDrag::DragDrop(_))
                                    && self.cancel_drag_drop()
                                {
                                    self.animation.reset_blink();
                                    dirty.mark_full();
                                    continue;
                                }

                                self.animation.reset_blink();

                                #[cfg(feature = "devtools")]
                                let _devtools_guard = self
                                    .focus
                                    .focused
                                    .filter(|id| self.core.tree.is_valid(*id))
                                    .is_some_and(|id| {
                                        self.core.tree.node_has_ancestor_with_key(
                                            id,
                                            crate::devtools::DEVTOOLS_KEY,
                                        )
                                    })
                                    .then(crate::debug::suppress_devtools_log);

                                crate::debug::internal_log!(
                                    "[tui-lipan] event: key {:?}",
                                    key.code
                                );

                                let key_result = self.dispatch_layered_key(key);
                                self.notify_focus_change();

                                #[cfg(feature = "devtools")]
                                {
                                    let effects: Vec<_> =
                                        self.framework_effects.drain(..).collect();
                                    for effect in effects {
                                        if matches!(effect, FrameworkSideEffect::ToggleDevtools) {
                                            let visible = self.devtools_state.borrow().visible;
                                            if self.set_devtools_visible(!visible) {
                                                dirty.mark_full();
                                            }
                                        }
                                    }
                                }
                                #[cfg(not(feature = "devtools"))]
                                self.framework_effects.clear();

                                if key_result.quit {
                                    crate::debug::internal_log!(
                                        "[tui-lipan] action: quit via key {:?}",
                                        key.code
                                    );
                                    self.core.ctx.quit();
                                    dirty.mark_full();
                                } else if let Some(level) = key_result.dirty_override {
                                    #[cfg(feature = "devtools")]
                                    self.apply_input_dirty(&mut dirty, level, "input:key");
                                    #[cfg(not(feature = "devtools"))]
                                    apply_dirty_level(&mut dirty, level);
                                } else if key_result.mark_full {
                                    #[cfg(feature = "devtools")]
                                    self.apply_input_dirty(
                                        &mut dirty,
                                        DirtyLevel::Full,
                                        "input:key",
                                    );
                                    #[cfg(not(feature = "devtools"))]
                                    dirty.mark_full();
                                } else if key_result.mark_layout {
                                    #[cfg(feature = "devtools")]
                                    self.apply_input_dirty(
                                        &mut dirty,
                                        DirtyLevel::LayoutOnly,
                                        "input:key",
                                    );
                                    #[cfg(not(feature = "devtools"))]
                                    dirty.mark_layout();
                                }

                                if key_result.terminal_shift_navigation {
                                    dirty.mark_layout();
                                }
                            }
                        }
                        CEvent::Paste(text) => {
                            self.animation.reset_blink();
                            if self.dispatch_focused_paste(text.as_str()) {
                                dirty.mark_layout();
                            }
                        }
                        CEvent::Mouse(m) if self.mouse_enabled => {
                            crate::debug::increment_mouse_events();

                            if let Some(mut mouse) = self.convert_mouse_event(m) {
                                let mut handled = false;

                                let needs_motion = self.needs_mouse_motion();
                                if matches!(mouse.kind, MouseKind::Moved) && !needs_motion {
                                    let (x, y) = self.to_content_coords(mouse.x, mouse.y);
                                    self.mouse.last_mouse = Some((x, y));
                                    handled = true;
                                }

                                if !handled && matches!(mouse.kind, MouseKind::Drag(_)) {
                                    let mut pending_non_drag: Option<MouseEvent> = None;
                                    while let Some(next_ev) = self.try_recv_event(event_rx)? {
                                        match next_ev {
                                            RunnerEvent::Terminal(CEvent::Mouse(next_m)) => {
                                                if let Some(next_mouse) =
                                                    self.convert_mouse_event(next_m)
                                                {
                                                    if matches!(next_mouse.kind, MouseKind::Drag(_))
                                                    {
                                                        mouse = next_mouse;
                                                    } else {
                                                        pending_non_drag = Some(next_mouse);
                                                        break;
                                                    }
                                                }
                                            }
                                            _ => {
                                                preserve_pending_event(&mut pending_event, next_ev);
                                                break;
                                            }
                                        }
                                    }

                                    // Dispatch the (possibly coalesced) drag event.
                                    // This is important for forwarding drag events to
                                    // terminal PTY applications that have mouse mode enabled.
                                    let drag_before = effective_active_drag_dirty_level(&self.drag);
                                    if self.dispatch_mouse(mouse) {
                                        let level = mouse_dispatch_dirty_level(
                                            mouse.kind,
                                            drag_before,
                                            effective_active_drag_dirty_level(&self.drag),
                                        );
                                        #[cfg(feature = "devtools")]
                                        self.apply_input_dirty(&mut dirty, level, "input:drag");
                                        #[cfg(not(feature = "devtools"))]
                                        apply_dirty_level(&mut dirty, level);
                                    }

                                    if let Some(non_drag) = pending_non_drag {
                                        let drag_before =
                                            effective_active_drag_dirty_level(&self.drag);
                                        if self.dispatch_mouse(non_drag) {
                                            let level = mouse_dispatch_dirty_level(
                                                non_drag.kind,
                                                drag_before,
                                                effective_active_drag_dirty_level(&self.drag),
                                            );
                                            #[cfg(feature = "devtools")]
                                            self.apply_input_dirty(&mut dirty, level, "input:drag");
                                            #[cfg(not(feature = "devtools"))]
                                            apply_dirty_level(&mut dirty, level);
                                        }
                                    }
                                    handled = true;
                                }

                                if !handled {
                                    if matches!(mouse.kind, MouseKind::Moved) {
                                        let mut dispatched_coalesced_move = false;
                                        while let Some(next_ev) = self.try_recv_event(event_rx)? {
                                            if let RunnerEvent::Terminal(CEvent::Mouse(next_m)) =
                                                next_ev
                                            {
                                                if let Some(next_mouse) =
                                                    self.convert_mouse_event(next_m)
                                                {
                                                    if matches!(next_mouse.kind, MouseKind::Moved) {
                                                        mouse = next_mouse;
                                                    } else {
                                                        if self.dispatch_mouse(mouse) {
                                                            dirty.mark_paint();
                                                        }
                                                        dispatched_coalesced_move = true;
                                                        let drag_before = active_drag_dirty_level(
                                                            &self.drag.active,
                                                        );
                                                        if self.dispatch_mouse(next_mouse) {
                                                            let level = mouse_dispatch_dirty_level(
                                                                next_mouse.kind,
                                                                drag_before,
                                                                active_drag_dirty_level(
                                                                    &self.drag.active,
                                                                ),
                                                            );
                                                            #[cfg(feature = "devtools")]
                                                            self.apply_input_dirty(
                                                                &mut dirty,
                                                                level,
                                                                "input:mouse",
                                                            );
                                                            #[cfg(not(feature = "devtools"))]
                                                            apply_dirty_level(&mut dirty, level);
                                                        }
                                                        break;
                                                    }
                                                }
                                            } else {
                                                preserve_pending_event(&mut pending_event, next_ev);
                                                break;
                                            }
                                        }
                                        if !dispatched_coalesced_move && self.dispatch_mouse(mouse)
                                        {
                                            dirty.mark_paint();
                                        }
                                    } else if matches!(
                                        mouse.kind,
                                        MouseKind::ScrollUp | MouseKind::ScrollDown
                                    ) {
                                        // Coalesce consecutive scroll events of the
                                        // same direction into a single dispatch.  A
                                        // fast scroll wheel can emit many events
                                        // between frames; processing each one
                                        // individually triggers a full render per tick.
                                        let mut count: u16 = 1;
                                        let direction = mouse.kind;
                                        while let Some(next_ev) = self.try_recv_event(event_rx)? {
                                            if let RunnerEvent::Terminal(CEvent::Mouse(next_m)) =
                                                next_ev
                                            {
                                                if let Some(next_mouse) =
                                                    self.convert_mouse_event(next_m)
                                                {
                                                    if next_mouse.kind == direction {
                                                        count = count.saturating_add(1);
                                                        // Keep the latest position
                                                        mouse = MouseEvent {
                                                            kind: direction,
                                                            ..next_mouse
                                                        };
                                                    } else {
                                                        // Different event - dispatch the
                                                        // coalesced scroll first, then handle
                                                        // the non-scroll event.
                                                        if self.dispatch_mouse_scroll(mouse, count)
                                                        {
                                                            // Scroll offset changed - re-reconcile
                                                            // with the cached element tree so newly
                                                            // visible children are laid out, but skip
                                                            // the expensive view() rebuild.
                                                            dirty.mark_layout();
                                                            #[cfg(feature = "devtools")]
                                                            self.note_attribution(
                                                                crate::devtools::state::UpdateSource::Input(
                                                                    "input:scroll",
                                                                ),
                                                                DirtyLevel::LayoutOnly,
                                                            );
                                                        }
                                                        if self.dispatch_mouse(next_mouse) {
                                                            #[cfg(feature = "devtools")]
                                                            self.apply_input_dirty(
                                                                &mut dirty,
                                                                DirtyLevel::Full,
                                                                "input:mouse",
                                                            );
                                                            #[cfg(not(feature = "devtools"))]
                                                            dirty.mark_full();
                                                        }
                                                        count = 0; // Already dispatched
                                                        break;
                                                    }
                                                }
                                            } else {
                                                preserve_pending_event(&mut pending_event, next_ev);
                                                break;
                                            }
                                        }
                                        if count > 0 && self.dispatch_mouse_scroll(mouse, count) {
                                            // Scroll offset changed - re-reconcile
                                            // with the cached element tree so newly
                                            // visible children are laid out, but skip
                                            // the expensive view() rebuild.
                                            dirty.mark_layout();
                                            #[cfg(feature = "devtools")]
                                            self.note_attribution(
                                                crate::devtools::state::UpdateSource::Input(
                                                    "input:scroll",
                                                ),
                                                DirtyLevel::LayoutOnly,
                                            );
                                        }
                                    } else {
                                        let drag_before =
                                            effective_active_drag_dirty_level(&self.drag);
                                        if self.dispatch_mouse(mouse) {
                                            let level = mouse_dispatch_dirty_level(
                                                mouse.kind,
                                                drag_before,
                                                effective_active_drag_dirty_level(&self.drag),
                                            );
                                            #[cfg(feature = "devtools")]
                                            self.apply_input_dirty(
                                                &mut dirty,
                                                level,
                                                "input:mouse",
                                            );
                                            #[cfg(not(feature = "devtools"))]
                                            apply_dirty_level(&mut dirty, level);
                                        }
                                    }
                                }
                            }
                        }
                        CEvent::Resize(_, _) => {
                            host_color_refresh_quiet_until =
                                Some(deferred_host_color_refresh_deadline(Instant::now()));
                            // Coalesce consecutive resize events, keeping only
                            // the final dimensions.  A window drag can flood the
                            // channel with many Resize events between frames;
                            // processing each one individually triggers a full
                            // render per tick.
                            while let Some(next_ev) = self.try_recv_event(event_rx)? {
                                if matches!(next_ev, RunnerEvent::Terminal(CEvent::Resize(_, _))) {
                                    // Discard intermediate resize - the terminal
                                    // backend will query the real size on draw.
                                    continue;
                                }
                                // Non-resize event: stop coalescing and preserve
                                // the event for the next loop iteration.
                                preserve_pending_event(&mut pending_event, next_ev);
                                break;
                            }

                            // In inline mode, ratatui computes the next viewport from the
                            // previous cursor offset plus the live backend cursor. Repositioning
                            // the cursor before draw pins the viewport and breaks shell reflow
                            // following (fzf-style behavior).
                            if matches!(self.drag.active, ActiveDrag::Scrollbar(_)) {
                                self.drag.scrollbar_recalc = true;
                            }
                            #[cfg(feature = "image")]
                            {
                                let pause = Duration::from_millis(image_resize_pause_ms() as u64);
                                self.suspend_image_animations_for(pause);
                                image_support::suspend_image_rendering_for(pause);
                            }
                            if matches!(self.surface.mode(), SurfaceMode::InlineTranscript { .. }) {
                                self.surface.inline.transcript_expanded = true;
                                self.surface.inline.transcript_reset_pending = true;
                                self.surface.inline.expanded_live_viewport_height = 0;
                                self.surface.inline.last_terminal_size = (0, 0);
                                self.last_frame_snapshot = None;
                                self.scroll_diff_snapshot = None;
                                self.last_scroll_frames.clear();
                                crate::debug::internal_log!(
                                    "[tui-lipan] dirty: resize (transcript expand)"
                                );
                                dirty.mark_full();
                            } else {
                                crate::debug::internal_log!("[tui-lipan] dirty: resize");
                                dirty.mark_full();
                            }
                        }
                        _ => {}
                    }
                }

                self.drain_messages_and_commands(&mut dirty)?;

                if self.apply_framework_commands() {
                    dirty.mark_full();
                }

                // A FocusIn refresh is commonly followed by resize events. Keep
                // the request pending until queued events drain and focus/resize
                // activity has been quiet briefly. The legacy lane then performs
                // its blocking OSC 4/10/11 probe; Termina fullscreen runs hand
                // the request to their input worker for a typed OSC 10/11 query
                // while preserving the startup probe's resolved ANSI slots.
                let host_color_refresh = if pending_event.is_some() {
                    false
                } else if let Some(next) = self.try_recv_event(event_rx)? {
                    preserve_pending_event(&mut pending_event, next);
                    false
                } else if host_color_refresh_wait_remaining(
                    host_color_refresh_quiet_until,
                    Instant::now(),
                )
                .is_some()
                {
                    false
                } else {
                    host_color_refresh_quiet_until = None;
                    if platform_input
                        .route_host_color_refresh(self.take_host_terminal_color_refresh_request())
                    {
                        self.refresh_host_terminal_colors(!self.surface.is_inline(), true)
                    } else {
                        false
                    }
                };

                let ctx_repaint = self.core.take_full_repaint_request();
                let devtools_request = self.apply_pending_devtools_request();
                let handoff_repaint = take_handoff_full_repaint_request();
                let force_host_redraw =
                    ctx_repaint || handoff_repaint || devtools_request || host_color_refresh;
                if force_host_redraw {
                    dirty.mark_full();
                }

                if self.apply_pending_focus_request() {
                    dirty.mark_full();
                }

                if self.sync_mouse_capture_preference(&mut terminal)? {
                    dirty.mark_full();
                }

                if self.core.has_pending_transcript_entries() {
                    dirty.mark_paint();
                }

                if self.surface.is_transcript() && self.surface.inline.transcript_reset_pending {
                    dirty.mark_full();
                }

                // DevTools metrics for a frame are recorded *after* its panel was
                // built, so the last app frame's numbers are only shown once a
                // later frame rebuilds the panel. When an app frame leaves other
                // work pending the panel already advances on its own, so honor the
                // pending refresh only on an otherwise-idle iteration: schedule a
                // single full "catch-up" frame that rebuilds the panel with the
                // latest metrics. That frame is marked suppressed so it records no
                // metrics and cannot re-arm the flag (see `record_devtools_frame_metrics`).
                #[cfg(feature = "devtools")]
                {
                    let refresh = std::mem::take(&mut self.devtools_refresh_pending);
                    self.devtools_metrics_suppressed = refresh && dirty.level() == DirtyLevel::None;
                    if self.devtools_metrics_suppressed {
                        dirty.mark_full();
                    }
                }

                let frame_level = dirty.level();

                // When a Full render is pending but more input events are
                // already queued, skip the expensive render and process the
                // next event first.  This lets rapid input sequences (e.g.
                // holding an arrow key on a theme-preview list) converge to
                // the final state before paying for the view()/expand/
                // reconcile cycle.  Forced repaints are never skipped.
                if frame_level == DirtyLevel::Full
                    && !force_host_redraw
                    && !(self.surface.is_transcript()
                        && self.surface.inline.transcript_reset_pending)
                    && pending_event.is_none()
                    && let Some(next) = self.try_recv_event(event_rx)?
                {
                    preserve_pending_event(&mut pending_event, next);
                    deferred_full = true;
                    #[cfg(feature = "profiling-tracing")]
                    tracing::trace!(
                        target: "tui_lipan::perf",
                        "frame skipped: more input pending",
                    );
                    continue;
                }

                match frame_level {
                    DirtyLevel::Full => {
                        if force_host_redraw {
                            invalidate_previous_frame(&mut terminal);
                            self.last_frame_snapshot = None;
                            self.scroll_diff_snapshot = None;
                        }
                        self.render(&mut terminal)?;
                    }
                    DirtyLevel::LayoutOnly => self.render_layout_only(&mut terminal)?,
                    DirtyLevel::PaintOnly => {
                        self.render_paint_only(&mut terminal)?;
                    }
                    DirtyLevel::None => {}
                }

                if !matches!(frame_level, DirtyLevel::None) {
                    self.apply_pending_ui_snapshot_request()?;
                }

                #[cfg(feature = "profiling-tracing")]
                tracing::trace!(
                    target: "tui_lipan::perf",
                    frame_ms = frame_start.elapsed().as_secs_f64() * 1000.0,
                    dirty = ?frame_level
                );
            }

            if let Some(f) = &self.exit_view_fn {
                exit_element = Some((f)(&self.core.component, &self.core.ctx));
            }

            // Exit path has two branches:
            // - Transcript mode: finish_inline_exit replays the full committed
            //   document (history only, no live viewport), then if an exit view
            //   was provided it is appended as the final transcript entry.
            // - Ephemeral / Fullscreen: finish_inline_exit positions the cursor
            //   below the viewport and prints a newline to scroll it into view.
            let include_live_viewport = exit_element.is_none();

            self.terminal.reset_cursor(terminal.backend_mut())?;
            self.finish_inline_exit(&mut terminal, include_live_viewport)?;

            if self.surface.is_transcript()
                && let Some(element) = exit_element.take()
            {
                self.clear_inline_transcript_surface_for_exit(&mut terminal)?;
                self.flush_inline_element_commit(&mut terminal, element)?;
                terminal.backend_mut().flush()?;
            }

            self.core.component.unmount(&mut self.core.ctx);
            Ok(())
        })();

        panic_keyboard_enhancement.store(false, Ordering::SeqCst);
        panic_theme_notifications.store(false, Ordering::SeqCst);

        if result.is_ok()
            && let Some(element) = exit_element
            && let Err(err) = exit_view::render(element, contrast_policy, self.terminal_bg)
        {
            crate::debug::internal_log!("[tui-lipan] exit view error: {}", err);
        }

        if let Err(err) = &result {
            crate::debug::internal_log!("[tui-lipan] error: {}", err);
        }

        #[cfg(feature = "devtools")]
        crate::debug::clear_devtools_log_sink();

        result
    }

    /// Set focus to the given node, updating the focused id, key, tag, and
    /// resetting the blink animation so the cursor is immediately visible.
    pub(crate) fn set_focus(&mut self, id: NodeId) {
        self.focus.focused = Some(id);
        self.focus.focused_key = self.core.tree.node(id).key.clone();
        self.focus.focused_tag = Some(crate::layout::tag::tag_of_node(self.core.tree.node(id)));
        self.animation.reset_blink();
    }

    pub(crate) fn content_bounds(&self, width: u16, height: u16) -> Rect {
        self.surface.content_bounds(width, height)
    }

    pub(crate) fn convert_mouse_event(
        &self,
        event: crossterm::event::MouseEvent,
    ) -> Option<MouseEvent> {
        let mouse = to_mouse_event(event)?;
        self.surface
            .convert_mouse_event(mouse, self.surface.inline.viewport_metrics)
    }

    pub(crate) fn set_viewport_metrics(&mut self, area: ratatui::layout::Rect) {
        self.surface.inline.viewport_metrics = ViewportMetrics {
            x: area.x,
            y: area.y,
            width: area.width,
            height: area.height,
        };
        if self.surface.is_transcript() && self.surface.inline.transcript_expanded {
            self.surface.inline.expanded_live_viewport_height = area.height.max(1);
        }
    }

    fn finish_inline_exit(
        &mut self,
        terminal: &mut crate::backend::ratatui_backend::Terminal,
        include_live_viewport: bool,
    ) -> Result<()> {
        if !self.surface.is_inline() || self.surface.inline.viewport_metrics.height == 0 {
            return Ok(());
        }

        if self.surface.is_transcript() {
            self.replay_inline_transcript_document(terminal, include_live_viewport)?;
            terminal.backend_mut().flush()?;
            return Ok(());
        }

        let size = terminal.size()?;
        if size.height == 0 {
            return Ok(());
        }

        let below_viewport = self
            .surface
            .inline
            .viewport_metrics
            .y
            .saturating_add(self.surface.inline.viewport_metrics.height);
        let row = below_viewport.min(size.height.saturating_sub(1));

        execute!(terminal.backend_mut(), MoveTo(0, row), Print("\n"))?;
        terminal.backend_mut().flush()?;
        Ok(())
    }

    fn recv_event(
        &self,
        timeout: Duration,
        event_rx: Option<&mpsc::Receiver<RunnerEvent>>,
    ) -> Result<Option<RunnerEvent>> {
        if self.surface.is_inline() {
            if crossterm::event::poll(timeout)? {
                return Ok(Some(RunnerEvent::Terminal(crossterm::event::read()?)));
            }
            return Ok(None);
        }

        let Some(rx) = event_rx else {
            return Ok(None);
        };
        match rx.recv_timeout(timeout) {
            Ok(event) => Ok(Some(event)),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "fullscreen input worker disconnected",
            )
            .into()),
        }
    }

    fn try_recv_event(
        &self,
        event_rx: Option<&mpsc::Receiver<RunnerEvent>>,
    ) -> Result<Option<RunnerEvent>> {
        if self.surface.is_inline() {
            if crossterm::event::poll(Duration::from_millis(0))? {
                return Ok(Some(RunnerEvent::Terminal(crossterm::event::read()?)));
            }
            return Ok(None);
        }

        let Some(rx) = event_rx else {
            return Ok(None);
        };
        match rx.try_recv() {
            Ok(event) => Ok(Some(event)),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "fullscreen input worker disconnected",
            )
            .into()),
        }
    }
}

#[cfg(feature = "image")]
pub(super) fn image_tick_floor_ms() -> u32 {
    static VALUE: OnceLock<u32> = OnceLock::new();
    *VALUE.get_or_init(|| {
        let fps = std::env::var("TUI_LIPAN_IMAGE_MAX_FPS")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(30)
            .max(1);
        (1_000 / fps).max(1)
    })
}

#[cfg(feature = "image")]
pub(super) fn image_tick_catchup_cap_ms() -> u32 {
    static VALUE: OnceLock<u32> = OnceLock::new();
    *VALUE.get_or_init(|| {
        std::env::var("TUI_LIPAN_IMAGE_MAX_CATCHUP_MS")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(100)
            .max(1)
    })
}

#[cfg(feature = "image")]
pub(super) fn image_resize_pause_ms() -> u32 {
    static VALUE: OnceLock<u32> = OnceLock::new();
    *VALUE.get_or_init(|| {
        std::env::var("TUI_LIPAN_IMAGE_RESIZE_PAUSE_MS")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(180)
            .max(1)
    })
}

#[cfg(feature = "image")]
pub(super) fn image_layout_stabilize_ms() -> u32 {
    static VALUE: OnceLock<u32> = OnceLock::new();
    *VALUE.get_or_init(|| {
        std::env::var("TUI_LIPAN_IMAGE_LAYOUT_STABILIZE_MS")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(120)
            .max(1)
    })
}

#[cfg(test)]
mod run_tests;
