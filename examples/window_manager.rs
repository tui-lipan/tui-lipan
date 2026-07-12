//! Hyprland-like mini window manager built from `Canvas` + normal widgets.
//!
//! Run with: cargo run --example window_manager
//!
//! Practical terminal "Mod" is Alt:
//! - Mod+h/j/k/l: spatial focus
//! - Mod+Enter: spawn window
//! - Mod+w: close focused window
//! - Mod+t: toggle floating/tiling
//! - Mod+f: toggle fullscreen
//! - Mod+Space: flip focused dwindle split
//! - Mod+1..9: switch workspace
//! - Mod+Shift+1..9: move focused window to workspace
//! - Mod+[ / Mod+]: shrink/grow focused dwindle split ratio
//!
//! Hovering focuses windows by default. Alt+left-drag any window from anywhere inside it:
//! floating windows move freely, while tiled windows temporarily reflow and then split
//! the target tile where they are dropped. Tiled drag/drop reuses the same geometry
//! transition channel as tile/float toggles, while the drag preview uses the remembered
//! floating size. Alt+right-drag resizes (floating windows resize from the nearest
//! corner; tiled windows adjust their dwindle split). Toggling a floating window back
//! to tiled inserts it under its current center. WM focus and
//! tui-lipan focus complement each other, so focused child widgets also focus their
//! containing window. The
//! example intentionally stays app-level: it composes
//! `Canvas`, `MouseRegion`, `Frame`, `List`, `Input`, `TextArea`, and other widgets
//! rather than adding a new primitive.

use std::cell::Cell;
use std::time::Duration;

use tui_lipan::prelude::*;

type WindowId = u32;

const WORKSPACE_COUNT: usize = 9;
const GEOMETRY_MS: u64 = 220;
const CLOSE_MS: u64 = 120;
const OPEN_DELAY_MS: u64 = 36;
const FOCUS_CHROME_MS: u64 = 160;
const TOP_BAR_HEIGHT: u16 = 3;
const TILE_GAP: f32 = 1.0;
const OUTER_GAP: f32 = 1.0;
/// Minimum cells of a floating window kept on each axis when it is dragged off the
/// terminal edges, so it can always be grabbed and pulled back into view.
const OFFSCREEN_MIN_VISIBLE: f32 = 6.0;
const DEFAULT_RATIO: f32 = 0.58;
const MIN_SPLIT_RATIO: f32 = 0.20;
const MAX_SPLIT_RATIO: f32 = 0.80;
const RATIO_STEP: f32 = 0.04;
/// Weights tile width against height when choosing a dwindle split direction (Hyprland's
/// `split_width_multiplier`). 2.0 corrects for terminal cells being ~twice as tall as wide,
/// so the comparison reflects visual squareness rather than raw cell counts.
const SPLIT_WIDTH_MULTIPLIER: f32 = 2.0;
const FOCUS_MODE_CHOICES: [WindowFocusMode; 3] = [
    WindowFocusMode::TitleClick,
    WindowFocusMode::WindowClick,
    WindowFocusMode::Hover,
];

#[derive(Default)]
struct WindowManagerDemo {
    config: WindowManagerConfig,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WindowFocusMode {
    TitleClick,
    WindowClick,
    Hover,
}

impl WindowFocusMode {
    fn label(self) -> &'static str {
        match self {
            Self::TitleClick => "title-click",
            Self::WindowClick => "window-click",
            Self::Hover => "hover",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct WindowAnimationConfig {
    enabled: bool,
    spawn: bool,
    close: bool,
    fullscreen: bool,
    tile_float: bool,
    axis_change: bool,
    focus_chrome: bool,
    geometry_duration: Duration,
    close_duration: Duration,
    focus_chrome_duration: Duration,
    open_delay: Duration,
}

impl Default for WindowAnimationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            spawn: true,
            close: true,
            fullscreen: true,
            tile_float: true,
            axis_change: true,
            focus_chrome: true,
            geometry_duration: Duration::from_millis(GEOMETRY_MS),
            close_duration: Duration::from_millis(CLOSE_MS),
            focus_chrome_duration: Duration::from_millis(FOCUS_CHROME_MS),
            open_delay: Duration::from_millis(OPEN_DELAY_MS),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct WindowManagerConfig {
    focus_mode: WindowFocusMode,
    show_titles: bool,
    animations: WindowAnimationConfig,
}

impl Default for WindowManagerConfig {
    fn default() -> Self {
        Self {
            focus_mode: WindowFocusMode::Hover,
            show_titles: true,
            animations: WindowAnimationConfig::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SplitAxis {
    Horizontal,
    Vertical,
}

impl SplitAxis {
    fn flipped(self) -> Self {
        match self {
            Self::Horizontal => Self::Vertical,
            Self::Vertical => Self::Horizontal,
        }
    }

    fn at_depth(self, depth: usize) -> Self {
        if depth.is_multiple_of(2) {
            self
        } else {
            self.flipped()
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Horizontal => "horizontal-first",
            Self::Vertical => "vertical-first",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Direction {
    Left,
    Down,
    Up,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GeometryAnimation {
    None,
    Spawn,
    Close,
    Fullscreen,
    TileFloat,
    AxisChange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResizeCorner {
    UpperLeft,
    UpperRight,
    LowerLeft,
    LowerRight,
}

impl ResizeCorner {
    fn label(self) -> &'static str {
        match self {
            Self::UpperLeft => "upper-left",
            Self::UpperRight => "upper-right",
            Self::LowerLeft => "lower-left",
            Self::LowerRight => "lower-right",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ResizeSession {
    id: WindowId,
    corner: ResizeCorner,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct MoveSession {
    id: WindowId,
    was_floating: bool,
    drag_rect: FloatRect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DemoApp {
    Terminal,
    Files,
    Metrics,
    Chat,
    Editor,
    Logs,
    Music,
    Tasks,
    Browser,
}

impl DemoApp {
    fn for_index(index: usize) -> Self {
        match index % 9 {
            0 => Self::Terminal,
            1 => Self::Files,
            2 => Self::Metrics,
            3 => Self::Chat,
            4 => Self::Editor,
            5 => Self::Logs,
            6 => Self::Music,
            7 => Self::Tasks,
            _ => Self::Browser,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Terminal => "Terminal",
            Self::Files => "Files",
            Self::Metrics => "Metrics",
            Self::Chat => "Chat",
            Self::Editor => "Editor",
            Self::Logs => "Logs",
            Self::Music => "Music",
            Self::Tasks => "Tasks",
            Self::Browser => "Browser",
        }
    }

    fn has_focusable_content(self) -> bool {
        matches!(
            self,
            Self::Files | Self::Chat | Self::Editor | Self::Logs | Self::Tasks | Self::Browser
        )
    }
}

#[derive(Clone, Debug)]
struct WindowState {
    id: WindowId,
    title: String,
    app: DemoApp,
    floating: bool,
    fullscreen: bool,
    floating_rect: FloatRect,
    opening: bool,
    closing: bool,
}

#[derive(Clone, Debug)]
struct Workspace {
    name: String,
    windows: Vec<WindowState>,
    tile_tree: Option<DwindleTree>,
    focused_window: Option<WindowId>,
    start_axis: SplitAxis,
    split_ratios: Vec<f32>,
}

impl Workspace {
    fn new(index: usize) -> Self {
        Self {
            name: format!("ws{}", index + 1),
            windows: Vec::new(),
            tile_tree: None,
            focused_window: None,
            start_axis: if index.is_multiple_of(2) {
                SplitAxis::Horizontal
            } else {
                SplitAxis::Vertical
            },
            split_ratios: vec![DEFAULT_RATIO; 16],
        }
    }

    fn visible_count(&self) -> usize {
        self.windows.iter().filter(|window| !window.closing).count()
    }

    fn tiled_ids(&self) -> Vec<WindowId> {
        let active = self.active_tiled_ids_by_window_order();
        let mut ordered = Vec::new();
        if let Some(tree) = self.tile_tree.as_ref() {
            collect_tree_leaves(tree, &mut ordered);
            ordered.retain(|id| active.contains(id));
            for id in &active {
                if !ordered.contains(id) {
                    ordered.push(*id);
                }
            }
        }

        if ordered.is_empty() { active } else { ordered }
    }

    fn active_tiled_ids_by_window_order(&self) -> Vec<WindowId> {
        self.windows
            .iter()
            .filter(|window| !window.floating && !window.closing)
            .map(|window| window.id)
            .collect()
    }
}

struct State {
    workspaces: Vec<Workspace>,
    active_workspace: usize,
    focused_window: Option<WindowId>,
    next_window_id: WindowId,
    status: String,
    moving_window: Option<MoveSession>,
    resizing_window: Option<ResizeSession>,
    animation: GeometryAnimation,
    last_viewport: Cell<Option<Rect>>,
}

#[derive(Clone, Debug)]
enum Msg {
    FocusWindow(WindowId, FrameworkFocus),
    HoverWindow(WindowId),
    BeginMove(WindowId, FloatRect, u16, u16, u16, u16, bool),
    MoveWindow(WindowId, i16, i16, bool),
    EndMove(WindowId, u16, u16),
    BeginResize(WindowId, ResizeCorner, bool),
    ResizeWindow(WindowId, ResizeCorner, i16, i16, bool),
    EndResize(WindowId),
    FinishOpen(WindowId),
    PruneClosed(WindowId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FrameworkFocus {
    Preserve,
    Request,
}

#[derive(Clone, Debug, PartialEq)]
enum DwindleTree {
    Leaf(WindowId),
    Split {
        axis: SplitAxis,
        ratio: f32,
        first: Box<DwindleTree>,
        second: Box<DwindleTree>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct WindowPlacement {
    id: WindowId,
    rect: FloatRect,
}

impl Component for WindowManagerDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        let mut workspaces: Vec<Workspace> = (0..WORKSPACE_COUNT).map(Workspace::new).collect();
        let mut next_window_id = 1;

        seed_window(
            &mut workspaces[0],
            &mut next_window_id,
            DemoApp::Terminal,
            false,
            FloatRect {
                x: 4.0,
                y: 3.0,
                w: 42.0,
                h: 12.0,
            },
        );
        seed_window(
            &mut workspaces[0],
            &mut next_window_id,
            DemoApp::Files,
            false,
            FloatRect {
                x: 49.0,
                y: 3.0,
                w: 34.0,
                h: 13.0,
            },
        );
        seed_window(
            &mut workspaces[0],
            &mut next_window_id,
            DemoApp::Metrics,
            false,
            FloatRect {
                x: 9.0,
                y: 16.0,
                w: 46.0,
                h: 13.0,
            },
        );
        seed_window(
            &mut workspaces[0],
            &mut next_window_id,
            DemoApp::Chat,
            true,
            FloatRect {
                x: 56.0,
                y: 11.0,
                w: 40.0,
                h: 14.0,
            },
        );

        seed_window(
            &mut workspaces[1],
            &mut next_window_id,
            DemoApp::Editor,
            false,
            FloatRect {
                x: 6.0,
                y: 4.0,
                w: 56.0,
                h: 18.0,
            },
        );
        seed_window(
            &mut workspaces[1],
            &mut next_window_id,
            DemoApp::Logs,
            false,
            FloatRect {
                x: 64.0,
                y: 5.0,
                w: 42.0,
                h: 16.0,
            },
        );

        seed_window(
            &mut workspaces[2],
            &mut next_window_id,
            DemoApp::Music,
            true,
            FloatRect {
                x: 8.0,
                y: 5.0,
                w: 44.0,
                h: 11.0,
            },
        );
        seed_window(
            &mut workspaces[2],
            &mut next_window_id,
            DemoApp::Tasks,
            false,
            FloatRect {
                x: 54.0,
                y: 6.0,
                w: 42.0,
                h: 14.0,
            },
        );

        let focused_window = workspaces[0].focused_window;
        State {
            workspaces,
            active_workspace: 0,
            focused_window,
            next_window_id,
            status:
                "Alt is Mod. Hover focuses windows. Alt+left-drag moves; Alt+right-drag resizes."
                    .to_string(),
            moving_window: None,
            resizing_window: None,
            animation: GeometryAnimation::None,
            last_viewport: Cell::new(None),
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::FocusWindow(id, framework_focus) => {
                focus_window(&mut ctx.state, id);
                if framework_focus == FrameworkFocus::Request {
                    request_window_focus(ctx, id);
                }
                ctx.state.status = format!("Focused window #{id}");
                Update::full()
            }
            Msg::HoverWindow(id) => {
                if self.config.focus_mode == WindowFocusMode::Hover
                    && ctx.state.focused_window != Some(id)
                {
                    focus_window(&mut ctx.state, id);
                    request_window_focus(ctx, id);
                    ctx.state.status = format!("Hover-focused window #{id}");
                    Update::full()
                } else {
                    Update::none()
                }
            }
            Msg::BeginMove(
                id,
                current_rect,
                from_local_x,
                from_local_y,
                target_w,
                target_h,
                modified,
            ) => {
                if !modified {
                    return Update::none();
                }
                focus_window(&mut ctx.state, id);
                request_window_focus(ctx, id);
                let bounds = canvas_bounds_from_viewport(ctx.viewport());
                let mut session = None;
                let mut status = None;
                if let Some(window) = active_window_mut(&mut ctx.state, id) {
                    window.opening = false;
                    if window.fullscreen {
                        status = Some(format!(
                            "Window #{id} is fullscreen; press Alt+F before moving it"
                        ));
                    } else {
                        let was_floating = window.floating;
                        let drag_rect = if was_floating {
                            current_rect
                        } else {
                            tiled_drag_preview_rect(
                                current_rect,
                                window.floating_rect,
                                bounds,
                                from_local_x,
                                from_local_y,
                                target_w,
                                target_h,
                            )
                        };
                        if was_floating {
                            window.floating_rect = drag_rect;
                        }
                        session = Some(MoveSession {
                            id,
                            was_floating,
                            drag_rect,
                        });
                        status = Some(if was_floating {
                            format!("Moving floating window #{id}")
                        } else {
                            format!("Dragging tiled window #{id}; drop over a tile to split it")
                        });
                    }
                }
                ctx.state.moving_window = session;
                ctx.state.animation = if session.is_some_and(|session| !session.was_floating) {
                    GeometryAnimation::TileFloat
                } else {
                    GeometryAnimation::None
                };
                if let Some(status) = status {
                    ctx.state.status = status;
                }
                Update::full()
            }
            Msg::MoveWindow(id, dx, dy, modified) => {
                if !modified {
                    return Update::none();
                }
                let bounds = canvas_bounds_from_viewport(ctx.viewport());
                let mut persisted_floating_rect = None;
                if let Some(session) = ctx
                    .state
                    .moving_window
                    .as_mut()
                    .filter(|session| session.id == id)
                {
                    session.drag_rect.x += f32::from(dx);
                    session.drag_rect.y += f32::from(dy);
                    // Floating windows may be dragged off the edge (clipped); a tiled drag
                    // preview stays fully inside.
                    session.drag_rect = if session.was_floating {
                        clamp_floating_rect(session.drag_rect, bounds)
                    } else {
                        clamp_float_rect(session.drag_rect, bounds)
                    };
                    if session.was_floating {
                        persisted_floating_rect = Some(session.drag_rect);
                    }
                    ctx.state.animation = if session.was_floating {
                        GeometryAnimation::None
                    } else {
                        GeometryAnimation::TileFloat
                    };
                    ctx.state.status = if session.was_floating {
                        format!("Moving floating #{id} by {dx:+}, {dy:+}")
                    } else {
                        format!("Dragging tiled #{id} by {dx:+}, {dy:+}")
                    };
                }
                if let Some(rect) = persisted_floating_rect
                    && let Some(window) = active_window_mut(&mut ctx.state, id)
                {
                    window.floating_rect = rect;
                }
                Update::full()
            }
            Msg::EndMove(id, x, y) => {
                let session = ctx.state.moving_window.filter(|session| session.id == id);
                if session.is_some() {
                    ctx.state.moving_window = None;
                }
                if let Some(session) = session {
                    if session.was_floating {
                        if let Some(window) = active_window_mut(&mut ctx.state, id) {
                            window.floating_rect = session.drag_rect;
                        }
                        ctx.state.status = format!("Finished moving floating window #{id}");
                    } else {
                        let viewport = ctx.viewport();
                        drop_tiled_window_at(&mut ctx.state, id, x, y, viewport);
                    }
                } else {
                    ctx.state.status = format!("Finished moving window #{id}");
                }
                Update::full()
            }
            Msg::BeginResize(id, corner, modified) => {
                if !modified {
                    return Update::none();
                }
                ctx.state.animation = GeometryAnimation::None;
                focus_window(&mut ctx.state, id);
                request_window_focus(ctx, id);
                ctx.state.resizing_window = Some(ResizeSession { id, corner });
                ctx.state.status = format!("Resizing window #{id} from {}", corner.label());
                Update::full()
            }
            Msg::ResizeWindow(id, corner, dx, dy, modified) => {
                if !modified {
                    return Update::none();
                }
                ctx.state.animation = GeometryAnimation::None;
                let viewport = ctx.viewport();
                let corner = ctx
                    .state
                    .resizing_window
                    .filter(|session| session.id == id)
                    .map(|session| session.corner)
                    .unwrap_or(corner);
                resize_window(&mut ctx.state, id, corner, dx, dy, viewport);
                Update::full()
            }
            Msg::EndResize(id) => {
                if ctx
                    .state
                    .resizing_window
                    .is_some_and(|session| session.id == id)
                {
                    ctx.state.resizing_window = None;
                }
                ctx.state.status = format!("Finished resizing window #{id}");
                Update::full()
            }
            Msg::FinishOpen(id) => {
                if let Some(window) = find_window_mut(&mut ctx.state, id) {
                    window.opening = false;
                    ctx.state.animation = GeometryAnimation::Spawn;
                }
                Update::full()
            }
            Msg::PruneClosed(id) => {
                remove_window(&mut ctx.state, id);
                Update::full()
            }
        }
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        if key.is(KeyCode::Esc) || key.is(KeyCode::Char('q')) {
            ctx.quit();
            return KeyUpdate::handled(Update::full());
        }

        if !is_mod_key(key) {
            return KeyUpdate::unhandled(Update::none());
        }

        sync_focus_from_framework(ctx);

        if let Some((index, symbol_implies_shift)) = workspace_key(key) {
            if key.mods.shift || symbol_implies_shift {
                move_focused_to_workspace(&mut ctx.state, index);
            } else {
                switch_workspace(&mut ctx.state, index);
            }
            request_current_window_focus(ctx);
            return KeyUpdate::handled(Update::full());
        }

        let viewport = ctx.viewport();
        let update = match key.code {
            KeyCode::Enter => spawn_window(ctx, self.config.animations),
            KeyCode::Char('w') | KeyCode::Char('W') | KeyCode::Char('q') | KeyCode::Char('Q') => {
                close_focused_window(ctx, self.config.animations)
            }
            KeyCode::Char('t') | KeyCode::Char('T') => {
                toggle_tiling(ctx);
                Update::full()
            }
            KeyCode::Char('f') | KeyCode::Char('F') => toggle_fullscreen(ctx),
            KeyCode::Char(' ') => {
                toggle_focused_split_axis(&mut ctx.state);
                Update::full()
            }
            KeyCode::Char('h') | KeyCode::Char('H') | KeyCode::Left => {
                if let Some(id) = focus_in_direction(&mut ctx.state, Direction::Left, viewport) {
                    request_window_focus(ctx, id);
                }
                Update::full()
            }
            KeyCode::Char('j') | KeyCode::Char('J') | KeyCode::Down => {
                if let Some(id) = focus_in_direction(&mut ctx.state, Direction::Down, viewport) {
                    request_window_focus(ctx, id);
                }
                Update::full()
            }
            KeyCode::Char('k') | KeyCode::Char('K') | KeyCode::Up => {
                if let Some(id) = focus_in_direction(&mut ctx.state, Direction::Up, viewport) {
                    request_window_focus(ctx, id);
                }
                Update::full()
            }
            KeyCode::Char('l') | KeyCode::Char('L') | KeyCode::Right => {
                if let Some(id) = focus_in_direction(&mut ctx.state, Direction::Right, viewport) {
                    request_window_focus(ctx, id);
                }
                Update::full()
            }
            KeyCode::Char('[') | KeyCode::Char('-') => {
                adjust_focused_split_ratio(&mut ctx.state, -RATIO_STEP);
                Update::full()
            }
            KeyCode::Char(']') | KeyCode::Char('=') | KeyCode::Char('+') => {
                adjust_focused_split_ratio(&mut ctx.state, RATIO_STEP);
                Update::full()
            }
            _ => return KeyUpdate::unhandled(Update::none()),
        };

        KeyUpdate::handled(update)
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let viewport = ctx.viewport();
        let viewport_changed = ctx
            .state
            .last_viewport
            .replace(Some(viewport))
            .is_some_and(|previous| previous != viewport);
        let bounds = canvas_bounds_from_viewport(viewport);
        let workspace = &ctx.state.workspaces[ctx.state.active_workspace];
        let moving_tiled = ctx
            .state
            .moving_window
            .filter(|session| !session.was_floating)
            .map(|session| session.id);
        let placements = workspace_target_rects_excluding(workspace, bounds, moving_tiled);
        let effective_focus = effective_focused_window(ctx, workspace);
        let mut canvas = Canvas::new()
            .style(Style::new().bg(Color::rgb(10, 12, 18)))
            .height(Length::Flex(1));

        if workspace.windows.iter().all(|window| window.closing) {
            canvas = canvas.child_at(
                empty_workspace_rect(bounds).to_rect(),
                empty_workspace_panel(),
            );
        }

        for window in ordered_windows(workspace, effective_focus) {
            let base_rect = placement_for(&placements, window.id)
                .unwrap_or_else(|| clamp_float_rect(window.floating_rect, bounds));
            let moving = ctx
                .state
                .moving_window
                .filter(|session| session.id == window.id);
            let target_rect = if window.closing {
                close_rect(window.floating_rect)
            } else if let Some(session) = moving
                && !window.fullscreen
            {
                // Match the clamp used while dragging so a floating window keeps its
                // off-screen position instead of snapping back inside.
                if session.was_floating {
                    clamp_floating_rect(session.drag_rect, bounds)
                } else {
                    clamp_float_rect(session.drag_rect, bounds)
                }
            } else if window.fullscreen {
                bounds
            } else {
                // Spawned windows appear directly at their tiled slot (and fade in via
                // opacity); only the surrounding windows animate to make room.
                base_rect
            };
            let config = self.transition_config_for(ctx, window, viewport_changed);
            let animated_rect =
                ctx.transition(format!("wm-window-rect-{}", window.id), target_rect, config);

            canvas = canvas.child_at(
                animated_rect.to_rect(),
                self.window_element(ctx, window, animated_rect, effective_focus),
            );
        }

        VStack::new()
            .style(
                Style::new()
                    .bg(Color::rgb(10, 12, 18))
                    .fg(Color::rgb(220, 225, 235)),
            )
            .child(
                top_bar(&ctx.state, self.config, effective_focus)
                    .height(Length::Px(TOP_BAR_HEIGHT)),
            )
            .child(canvas)
            .into()
    }
}

impl WindowManagerDemo {
    fn transition_config_for(
        &self,
        ctx: &Context<Self>,
        window: &WindowState,
        viewport_changed: bool,
    ) -> TransitionConfig {
        if viewport_changed
            || ctx
                .state
                .moving_window
                .is_some_and(|session| session.id == window.id)
            || ctx
                .state
                .resizing_window
                .is_some_and(|session| session.id == window.id)
        {
            return instant_transition();
        }

        let animations = self.config.animations;
        if !animations.enabled {
            return instant_transition();
        }

        let enabled = match ctx.state.animation {
            GeometryAnimation::None => false,
            GeometryAnimation::Spawn => animations.spawn,
            GeometryAnimation::Close => animations.close,
            GeometryAnimation::Fullscreen => animations.fullscreen,
            GeometryAnimation::TileFloat => animations.tile_float,
            GeometryAnimation::AxisChange => animations.axis_change,
        };
        if !enabled {
            return instant_transition();
        }

        let duration = if window.closing {
            animations.close_duration
        } else {
            animations.geometry_duration
        };
        geometry_transition(duration)
    }

    fn window_opacity_config(&self, window: &WindowState) -> TransitionConfig {
        let animations = self.config.animations;
        if !animations.enabled {
            return instant_transition();
        }
        if window.closing {
            return if animations.close {
                TransitionConfig {
                    duration: animations.close_duration,
                    easing: Easing::EaseOutQuad,
                }
            } else {
                instant_transition()
            };
        }
        // Non-closing windows keep the spawn-fade config so the open-time 0 -> 1 flip
        // (driven by `opening` clearing in `FinishOpen`) animates instead of snapping.
        // A window's opacity only ever changes while opening or closing, so this never
        // produces a spurious fade.
        if animations.spawn {
            TransitionConfig {
                duration: animations.close_duration,
                easing: Easing::EaseOutQuad,
            }
        } else {
            instant_transition()
        }
    }

    fn focus_chrome_transition_config(&self) -> TransitionConfig {
        let animations = self.config.animations;
        if animations.enabled && animations.focus_chrome {
            TransitionConfig {
                duration: animations.focus_chrome_duration,
                easing: Easing::EaseInOutCubic,
            }
        } else {
            instant_transition()
        }
    }

    fn chrome_color(
        &self,
        ctx: &Context<Self>,
        window: WindowId,
        slot: &str,
        target: Color,
    ) -> Color {
        ctx.transition(
            format!("wm-window-chrome-{window}-{slot}"),
            target,
            self.focus_chrome_transition_config(),
        )
    }

    fn window_element(
        &self,
        ctx: &Context<Self>,
        window: &WindowState,
        animated_rect: FloatRect,
        effective_focus: Option<WindowId>,
    ) -> Element {
        let window_key = window_key(window.id);
        let focused = effective_focus == Some(window.id);
        let id = window.id;
        let title_prefix = if window.floating { "󰹙" } else { "󰖲" };
        let close_suffix = if window.closing { " closing" } else { "" };
        let fullscreen_suffix = if window.fullscreen { " fullscreen" } else { "" };
        let title = format!(
            " {title_prefix} #{} {}{}{} ",
            window.id, window.title, fullscreen_suffix, close_suffix
        );
        let border_style = if window.floating {
            BorderStyle::Double
        } else {
            BorderStyle::Rounded
        };
        let frame_fg = self.chrome_color(
            ctx,
            window.id,
            "frame-fg",
            if focused {
                Color::rgb(124, 207, 255)
            } else {
                Color::rgb(125, 135, 150)
            },
        );
        let frame_bg = self.chrome_color(
            ctx,
            window.id,
            "frame-bg",
            if focused {
                Color::rgb(18, 24, 34)
            } else {
                Color::rgb(15, 18, 26)
            },
        );
        let title_bar_bg = self.chrome_color(
            ctx,
            window.id,
            "title-bg",
            if focused {
                Color::rgb(124, 207, 255)
            } else {
                Color::rgb(35, 42, 56)
            },
        );
        let title_bar_fg = self.chrome_color(
            ctx,
            window.id,
            "title-fg",
            if focused {
                Color::rgb(15, 18, 26)
            } else {
                Color::rgb(175, 185, 202)
            },
        );

        let frame_style = Style::new().fg(frame_fg).bg(frame_bg);
        let title_bar_fill_style = Style::new()
            .bg(title_bar_bg)
            .contrast_policy(ContrastPolicy::Off);
        let title_bar_text_style = if focused {
            Style::new()
                .fg(title_bar_fg)
                .bold()
                .contrast_policy(ContrastPolicy::Off)
        } else {
            Style::new()
                .fg(title_bar_fg)
                .contrast_policy(ContrastPolicy::Off)
        };
        let title_bar: Option<Element> = self.config.show_titles.then(|| {
            let mut region = MouseRegion::new().capture_click(true);
            if self.config.focus_mode == WindowFocusMode::TitleClick {
                region = region.on_mouse_down(
                    ctx.link()
                        .callback(move |_| Msg::FocusWindow(id, FrameworkFocus::Request)),
                );
            }
            let title_line = format!(
                "{title:<1}  {} • {:.0}×{:.0} @ {:.0},{:.0}",
                if window.fullscreen {
                    "fullscreen"
                } else if window.floating {
                    "floating"
                } else {
                    "tiled"
                },
                animated_rect.w,
                animated_rect.h,
                animated_rect.x,
                animated_rect.y
            );
            region
                .child(
                    // Row chrome owns the background; Text only paints glyphs.
                    // This keeps titlebar slack cells highlighted across the full window width.
                    HStack::new()
                        .style(title_bar_fill_style)
                        .width(Length::Flex(1))
                        .height(Length::Px(1))
                        .child(
                            Text::new(title_line)
                                .style(title_bar_text_style)
                                .overflow(Overflow::Ellipsis)
                                .width(Length::Flex(1))
                                .height(Length::Px(1)),
                        ),
                )
                .into()
        });

        let mut body = Frame::new()
            .border(true)
            .border_style(border_style)
            .style(frame_style)
            .focus_style(Style::default())
            .padding((0, 1, 0, 1));
        if let Some(title_bar) = title_bar {
            body = body
                .decoration(titlebar_top_edge(title_bar_bg))
                .header(title_bar);
        }
        let body = if window.app.has_focusable_content() {
            body
        } else {
            body.focusable(true)
        };
        let body: Element = body
            .child(demo_content(window.id, window.app, focused))
            .into();
        let body = body.key(window_body_focus_key(window.id));

        let mut window_region = MouseRegion::new()
            .drag_requires_mods(KeyMods::ALT)
            .right_drag_requires_mods(KeyMods::ALT)
            .on_drag_start(ctx.link().callback(move |event: MouseDragEvent| {
                Msg::BeginMove(
                    id,
                    animated_rect,
                    event.from_local_x,
                    event.from_local_y,
                    event.target_w,
                    event.target_h,
                    event.mods.alt,
                )
            }))
            .on_drag(ctx.link().callback(move |event: MouseDragEvent| {
                Msg::MoveWindow(id, event.delta_x, event.delta_y, event.mods.alt)
            }))
            .on_drag_end(
                ctx.link()
                    .callback(move |event: MouseDragEvent| Msg::EndMove(id, event.x, event.y)),
            )
            .on_right_drag_start(ctx.link().callback(move |event: MouseDragEvent| {
                Msg::BeginResize(id, nearest_resize_corner(event), event.mods.alt)
            }))
            .on_right_drag(ctx.link().callback(move |event: MouseDragEvent| {
                Msg::ResizeWindow(
                    id,
                    nearest_resize_corner(event),
                    event.delta_x,
                    event.delta_y,
                    event.mods.alt,
                )
            }))
            .on_right_drag_end(ctx.link().callback(move |_| Msg::EndResize(id)));

        match self.config.focus_mode {
            WindowFocusMode::TitleClick if self.config.show_titles => {}
            WindowFocusMode::TitleClick | WindowFocusMode::WindowClick => {
                window_region = window_region.bubble_mouse_down(true).on_mouse_down(
                    ctx.link()
                        .callback(move |_| Msg::FocusWindow(id, FrameworkFocus::Preserve)),
                );
            }
            WindowFocusMode::Hover => {
                window_region =
                    window_region.on_mouse_move(ctx.link().callback(move |_| Msg::HoverWindow(id)));
            }
        }

        // Opening and closing windows fade between transparent and opaque (dissolving
        // into the canvas backdrop) at their destination rect. The key lives on the
        // Animated wrapper so its opacity state survives canvas reordering, and stays an
        // ancestor of the focusable body so `has_focus_within_key` still resolves.
        let opacity = if window.closing || window.opening {
            0.0
        } else {
            1.0
        };
        let element: Element = Animated::new(window_region.child(body))
            .opacity(opacity)
            .transition(self.window_opacity_config(window))
            .into();

        element.key(window_key)
    }
}

fn titlebar_top_edge(bg: Color) -> EdgeDecoration {
    EdgeDecoration::new(Edge::Top)
        .glyph(DecorationGlyph::Custom(' '))
        .cap_start(DecorationGlyph::Custom(' '))
        .cap_end(DecorationGlyph::Custom(' '))
        .style(
            Style::new()
                .fg(bg)
                .bg(bg)
                .contrast_policy(ContrastPolicy::Off),
        )
}

fn seed_window(
    workspace: &mut Workspace,
    next_window_id: &mut WindowId,
    app: DemoApp,
    floating: bool,
    floating_rect: FloatRect,
) {
    let id = *next_window_id;
    *next_window_id = next_window_id.saturating_add(1);
    workspace.windows.push(WindowState {
        id,
        title: app.title().to_string(),
        app,
        floating,
        fullscreen: false,
        floating_rect,
        opening: false,
        closing: false,
    });
    if !floating {
        append_tiled_window(workspace, id);
    }
    workspace.focused_window = Some(id);
}

fn append_tiled_window(workspace: &mut Workspace, id: WindowId) {
    if workspace
        .tile_tree
        .as_ref()
        .is_some_and(|tree| tree_contains(tree, id))
    {
        return;
    }
    workspace.tile_tree = Some(append_tiled_leaf(
        workspace.tile_tree.take(),
        id,
        workspace.start_axis,
    ));
}

fn remove_tiled_window(workspace: &mut Workspace, id: WindowId) {
    workspace.tile_tree = workspace
        .tile_tree
        .take()
        .and_then(|tree| remove_tree_leaf(tree, id).0);
}

fn geometry_transition(duration: Duration) -> TransitionConfig {
    TransitionConfig {
        duration,
        easing: Easing::EaseInOutCubic,
    }
}

fn instant_transition() -> TransitionConfig {
    TransitionConfig {
        duration: Duration::ZERO,
        easing: Easing::Linear,
    }
}

fn is_mod_key(key: KeyEvent) -> bool {
    key.mods.alt
}

fn workspace_key(key: KeyEvent) -> Option<(usize, bool)> {
    let (digit, symbol_implies_shift) = match key.code {
        KeyCode::Char('1') => (1, false),
        KeyCode::Char('2') => (2, false),
        KeyCode::Char('3') => (3, false),
        KeyCode::Char('4') => (4, false),
        KeyCode::Char('5') => (5, false),
        KeyCode::Char('6') => (6, false),
        KeyCode::Char('7') => (7, false),
        KeyCode::Char('8') => (8, false),
        KeyCode::Char('9') => (9, false),
        // Common shifted symbols from terminals that do not preserve Char('1') + shift.
        KeyCode::Char('!') => (1, true),
        KeyCode::Char('@') => (2, true),
        KeyCode::Char('#') => (3, true),
        KeyCode::Char('$') => (4, true),
        KeyCode::Char('%') => (5, true),
        KeyCode::Char('^') => (6, true),
        KeyCode::Char('&') => (7, true),
        KeyCode::Char('*') => (8, true),
        KeyCode::Char('(') => (9, true),
        _ => return None,
    };

    Some((digit - 1, symbol_implies_shift))
}

fn window_key(id: WindowId) -> String {
    format!("wm-window-{id}")
}

fn window_body_focus_key(id: WindowId) -> String {
    format!("wm-window-focus-{id}")
}

fn window_widget_focus_key(id: WindowId, role: &str) -> String {
    format!("wm-window-widget-{id}-{role}")
}

fn primary_window_focus_key(id: WindowId, app: DemoApp) -> String {
    match app {
        DemoApp::Files => window_widget_focus_key(id, "files"),
        DemoApp::Chat => window_widget_focus_key(id, "chat-input"),
        DemoApp::Editor => window_widget_focus_key(id, "editor"),
        DemoApp::Logs => window_widget_focus_key(id, "logs"),
        DemoApp::Tasks => window_widget_focus_key(id, "tasks"),
        DemoApp::Browser => window_widget_focus_key(id, "browser-input"),
        DemoApp::Terminal | DemoApp::Metrics | DemoApp::Music => window_body_focus_key(id),
    }
}

fn request_window_focus(ctx: &mut Context<WindowManagerDemo>, id: WindowId) {
    let key = ctx.state.workspaces[ctx.state.active_workspace]
        .windows
        .iter()
        .find(|window| window.id == id && !window.closing)
        .map(|window| primary_window_focus_key(window.id, window.app))
        .unwrap_or_else(|| window_body_focus_key(id));
    ctx.request_focus(key);
}

fn request_current_window_focus(ctx: &mut Context<WindowManagerDemo>) {
    if let Some(id) = ctx.state.focused_window {
        request_window_focus(ctx, id);
    }
}

fn framework_focused_window(
    ctx: &Context<WindowManagerDemo>,
    workspace: &Workspace,
) -> Option<WindowId> {
    workspace
        .windows
        .iter()
        .filter(|window| !window.closing)
        .find(|window| ctx.has_focus_within_key(window_key(window.id)))
        .map(|window| window.id)
}

fn effective_focused_window(
    ctx: &Context<WindowManagerDemo>,
    workspace: &Workspace,
) -> Option<WindowId> {
    framework_focused_window(ctx, workspace).or(ctx.state.focused_window)
}

fn sync_focus_from_framework(ctx: &mut Context<WindowManagerDemo>) {
    let framework_focus = {
        let workspace = &ctx.state.workspaces[ctx.state.active_workspace];
        framework_focused_window(ctx, workspace)
    };
    if let Some(id) = framework_focus {
        focus_window(&mut ctx.state, id);
    }
}

fn top_bar(
    state: &State,
    config: WindowManagerConfig,
    effective_focus: Option<WindowId>,
) -> VStack {
    let workspace = &state.workspaces[state.active_workspace];
    let workspace_strip = state.workspaces.iter().enumerate().fold(
        HStack::new().gap(1).height(Length::Px(1)),
        |row, (idx, workspace)| {
            let active = idx == state.active_workspace;
            let label = format!(
                " {}:{}{} ",
                idx + 1,
                workspace.visible_count(),
                if active { "*" } else { "" }
            );
            let style = if active {
                Style::new()
                    .fg(Color::rgb(15, 18, 26))
                    .bg(Color::rgb(124, 207, 255))
                    .bold()
            } else {
                Style::new()
                    .fg(Color::rgb(120, 130, 145))
                    .bg(Color::rgb(18, 24, 34))
            };
            row.child(Text::new(label).style(style).height(Length::Px(1)))
        },
    );

    let focus_label = effective_focus
        .map(|id| format!("#{id}"))
        .unwrap_or_else(|| "none".to_string());

    VStack::new()
        .style(Style::new().bg(Color::rgb(10, 12, 18)))
        .child(
            HStack::new()
                .gap(2)
                .height(Length::Px(1))
                .child(
                    Text::new(" Canvas WM ")
                        .style(
                            Style::new()
                                .fg(Color::rgb(240, 245, 255))
                                .bg(Color::rgb(57, 91, 162))
                                .bold(),
                        )
                        .height(Length::Px(1)),
                )
                .child(workspace_strip)
                .child(
                    Text::new(format!(
                        "active={} focused={} root={} focus={} titles={} anim={} modes={}",
                        workspace.name,
                        focus_label,
                        workspace_root_axis(workspace).label(),
                        config.focus_mode.label(),
                        if config.show_titles { "on" } else { "off" },
                        if config.animations.enabled { "on" } else { "off" },
                        focus_mode_choices_label(),
                    ))
                    .style(Style::new().fg(Color::rgb(150, 160, 176)))
                    .height(Length::Px(1)),
                ),
        )
        .child(
            Text::new("Mod=Alt • h/j/k/l focus • Enter spawn • w close • t tile/float • f fullscreen • Space flip split • Alt+drag move/resize • [/] ratio")
                .style(Style::new().fg(Color::rgb(155, 166, 185)))
                .height(Length::Px(1)),
        )
        .child(
            Text::new(state.status.clone())
                .style(Style::new().fg(Color::rgb(109, 213, 175)))
                .height(Length::Px(1)),
        )
}

fn focus_mode_choices_label() -> String {
    format!(
        "{}/{}/{}",
        FOCUS_MODE_CHOICES[0].label(),
        FOCUS_MODE_CHOICES[1].label(),
        FOCUS_MODE_CHOICES[2].label()
    )
}

fn workspace_root_axis(workspace: &Workspace) -> SplitAxis {
    match workspace.tile_tree.as_ref() {
        Some(DwindleTree::Split { axis, .. }) => *axis,
        _ => workspace.start_axis,
    }
}

fn demo_content(id: WindowId, app: DemoApp, focused: bool) -> Element {
    match app {
        DemoApp::Terminal => terminal_content(focused),
        DemoApp::Files => files_content(id),
        DemoApp::Metrics => metrics_content(),
        DemoApp::Chat => chat_content(id),
        DemoApp::Editor => editor_content(id),
        DemoApp::Logs => logs_content(id),
        DemoApp::Music => music_content(),
        DemoApp::Tasks => tasks_content(id),
        DemoApp::Browser => browser_content(id),
    }
}

fn terminal_content(focused: bool) -> Element {
    let prompt_style = if focused {
        Style::new().fg(Color::rgb(109, 213, 175)).bold()
    } else {
        Style::new().fg(Color::rgb(99, 170, 140))
    };

    VStack::new()
        .gap(1)
        .child(Text::new("$ hyprctl clients | jq '.[] | .title'").style(prompt_style))
        .child(
            Text::new("\"Terminal\"  \"Files\"  \"Metrics\"  \"Chat\"")
                .style(Style::new().fg(Color::rgb(196, 214, 255))),
        )
        .child(Text::new("$ cargo run --example window_manager").style(prompt_style))
        .child(Text::new(
            "Canvas hosts ordinary widgets at animated FloatRects.",
        ))
        .child(
            ProgressBar::new(0.72)
                .progress_style(ProgressStyle::Block)
                .height(Length::Px(1)),
        )
        .into()
}

fn files_content(id: WindowId) -> Element {
    let list: Element = List::new()
        .border(false)
        .height(Length::Flex(1))
        .selected(2)
        .selection_style(Style::new().bg(Color::rgb(48, 72, 100)).fg(Color::White))
        .items(vec![
            ListItem::header("project"),
            ListItem::new("󰉋 src/").description("framework"),
            ListItem::new("󰉋 examples/").description("demos"),
            ListItem::new("󰉋 docs/").description("guides"),
            ListItem::new("󰈙 Cargo.toml").description("workspace"),
            ListItem::new("󰈙 README.md").description("overview"),
        ])
        .into();
    list.key(window_widget_focus_key(id, "files"))
}

fn metrics_content() -> Element {
    VStack::new()
        .gap(1)
        .child(metric_row("CPU", 0.64, Color::rgb(124, 207, 255)))
        .child(metric_row("GPU", 0.48, Color::rgb(181, 137, 255)))
        .child(metric_row("RAM", 0.71, Color::rgb(109, 213, 175)))
        .child(Text::new("re-tile: FloatRect transition keys stay stable"))
        .into()
}

fn metric_row(label: &str, value: f64, color: Color) -> Element {
    HStack::new()
        .gap(1)
        .child(Text::new(format!("{label:>3}")).width(Length::Px(4)))
        .child(
            ProgressBar::new(value)
                .progress_style(ProgressStyle::Block)
                .filled_style(Style::new().fg(color))
                .height(Length::Px(1)),
        )
        .child(Text::new(format!("{:>3}%", (value * 100.0).round() as u16)))
        .into()
}

fn chat_content(id: WindowId) -> Element {
    let input: Element = Input::new("demo message")
        .prefix("> ")
        .placeholder("type here in a real app")
        .border(false)
        .style(Style::new().fg(Color::rgb(196, 214, 255)))
        .into();

    VStack::new()
        .gap(1)
        .child(Text::new("ada: the floating chat window is draggable"))
        .child(Text::new("lin: tiled panes reflow behind it"))
        .child(Text::new("you: try Alt+t, Alt+f, and Alt+[ / Alt+]"))
        .child(input.key(window_widget_focus_key(id, "chat-input")))
        .into()
}

fn editor_content(id: WindowId) -> Element {
    let editor: Element = TextArea::new(
        "fn split(rect, axis, ratio) {\n    // persistent target-split tree\n    // Mod+drag can split any tile\n}\n\n// Mod+Space flips the focused pair.",
    )
    .read_only(true)
    .border(false)
    .wrap(false)
    .height(Length::Flex(1))
    .into();
    editor.key(window_widget_focus_key(id, "editor"))
}

fn logs_content(id: WindowId) -> Element {
    let list: Element = List::new()
        .border(false)
        .height(Length::Flex(1))
        .selection_symbol(Some("›"))
        .selected(4)
        .items(vec![
            ListItem::new("[00:00] compositor booted"),
            ListItem::new("[00:01] workspace 1 mapped"),
            ListItem::new("[00:02] Canvas reconcile pass"),
            ListItem::new("[00:03] mouse region drag armed"),
            ListItem::new("[00:04] transition tick 16ms"),
            ListItem::new("[00:05] focus moved spatially"),
        ])
        .into();
    list.key(window_widget_focus_key(id, "logs"))
}

fn music_content() -> Element {
    VStack::new()
        .gap(1)
        .child(Text::new("Now playing: Wondrous Flame"))
        .child(
            ProgressBar::new(0.38)
                .progress_style(ProgressStyle::Block)
                .height(Length::Px(1)),
        )
        .child(Text::new("vol 68%  repeat: workspace"))
        .child(Text::new("♪  ▂▃▅▇▆▃▂  ▃▅▇▅▃▁"))
        .into()
}

fn tasks_content(id: WindowId) -> Element {
    let list: Element = List::new()
        .border(false)
        .height(Length::Flex(1))
        .selection_full_width(true)
        .selected(1)
        .items(vec![
            ListItem::new("☑ add Canvas primitive"),
            ListItem::new("☑ wire FloatRect Lerp"),
            ListItem::new("☐ polish drag-resize handles"),
            ListItem::new("☐ write docs example catalog"),
        ])
        .into();
    list.key(window_widget_focus_key(id, "tasks"))
}

fn browser_content(id: WindowId) -> Element {
    let input: Element = Input::new("https://example.local/canvas-wm")
        .prefix("⌕ ")
        .border(false)
        .into();

    VStack::new()
        .gap(1)
        .child(input.key(window_widget_focus_key(id, "browser-input")))
        .child(Text::new("Canvas item rectangles are local to the Canvas."))
        .child(Text::new(
            "Normal widgets still own focus, hover, and layout inside each rect.",
        ))
        .child(
            ProgressBar::new(0.91)
                .progress_style(ProgressStyle::Block)
                .height(Length::Px(1)),
        )
        .into()
}

fn empty_workspace_panel() -> Element {
    Frame::new()
        .title(" Empty workspace ")
        .border(true)
        .border_style(BorderStyle::Rounded)
        .style(
            Style::new()
                .fg(Color::rgb(130, 145, 165))
                .bg(Color::rgb(15, 18, 26)),
        )
        .padding(1)
        .child(
            VStack::new()
                .gap(1)
                .child(Text::new("No windows here yet."))
                .child(Text::new("Press Mod+Enter to spawn a demo window.")),
        )
        .into()
}

fn canvas_bounds_from_viewport(viewport: Rect) -> FloatRect {
    FloatRect {
        x: 0.0,
        y: 0.0,
        w: f32::from(viewport.w),
        h: f32::from(viewport.h.saturating_sub(TOP_BAR_HEIGHT)),
    }
}

fn empty_workspace_rect(bounds: FloatRect) -> FloatRect {
    let w = bounds.w.min(46.0).max(bounds.w.min(18.0));
    let h = bounds.h.min(8.0).max(bounds.h.min(4.0));
    FloatRect {
        x: bounds.x + ((bounds.w - w) / 2.0).max(0.0),
        y: bounds.y + ((bounds.h - h) / 2.0).max(0.0),
        w,
        h,
    }
}

fn workspace_target_rects(workspace: &Workspace, bounds: FloatRect) -> Vec<WindowPlacement> {
    workspace_target_rects_excluding(workspace, bounds, None)
}

fn workspace_target_rects_excluding(
    workspace: &Workspace,
    bounds: FloatRect,
    exclude_tiled: Option<WindowId>,
) -> Vec<WindowPlacement> {
    let mut placements = Vec::new();
    let tile_bounds = inset_float_rect(bounds, OUTER_GAP);
    if let Some(tree) = effective_tile_tree(workspace, exclude_tiled) {
        allocate_dwindle(&tree, tile_bounds, TILE_GAP, &mut placements);
    }

    for window in workspace
        .windows
        .iter()
        .filter(|window| window.floating && !window.closing)
    {
        placements.push(WindowPlacement {
            // Floating windows keep any off-screen position they were dragged to (the
            // Canvas clips the overflow); only the grabbable margin is enforced.
            id: window.id,
            rect: clamp_floating_rect(window.floating_rect, bounds),
        });
    }

    placements
}

fn effective_tile_tree(
    workspace: &Workspace,
    exclude_tiled: Option<WindowId>,
) -> Option<DwindleTree> {
    let active_ids: Vec<WindowId> = workspace
        .active_tiled_ids_by_window_order()
        .into_iter()
        .filter(|id| Some(*id) != exclude_tiled)
        .collect();
    if active_ids.is_empty() {
        return None;
    }

    let mut tree = workspace
        .tile_tree
        .clone()
        .and_then(|tree| prune_tree_to_ids(tree, &active_ids))
        .or_else(|| build_dwindle_tree(&active_ids, workspace.start_axis, &workspace.split_ratios));

    for id in active_ids {
        if !tree.as_ref().is_some_and(|tree| tree_contains(tree, id)) {
            tree = Some(append_tiled_leaf(tree, id, workspace.start_axis));
        }
    }

    tree
}

fn placement_for(placements: &[WindowPlacement], id: WindowId) -> Option<FloatRect> {
    placements
        .iter()
        .find(|placement| placement.id == id)
        .map(|placement| placement.rect)
}

fn ordered_windows(workspace: &Workspace, focused: Option<WindowId>) -> Vec<&WindowState> {
    let mut windows: Vec<&WindowState> = workspace.windows.iter().collect();
    windows.sort_by_key(|window| {
        (
            window_z_group(window),
            window.fullscreen,
            focused == Some(window.id),
            window.id,
        )
    });
    windows
}

fn window_z_group(window: &WindowState) -> u8 {
    match (window.closing, window.floating) {
        // Tiled close animations should not cover the windows expanding into their space.
        (true, false) => 0,
        (false, false) => 1,
        (false, true) => 2,
        // Floating windows do not resize the tile layout, so keep their fade-out above it.
        (true, true) => 3,
    }
}

fn build_dwindle_tree(
    ids: &[WindowId],
    start_axis: SplitAxis,
    ratios: &[f32],
) -> Option<DwindleTree> {
    build_dwindle_tree_at(ids, start_axis, ratios, 0)
}

fn build_dwindle_tree_at(
    ids: &[WindowId],
    start_axis: SplitAxis,
    ratios: &[f32],
    depth: usize,
) -> Option<DwindleTree> {
    match ids {
        [] => None,
        [id] => Some(DwindleTree::Leaf(*id)),
        [first, rest @ ..] => Some(DwindleTree::Split {
            axis: start_axis.at_depth(depth),
            ratio: ratio_at(ratios, depth),
            first: Box::new(DwindleTree::Leaf(*first)),
            second: Box::new(build_dwindle_tree_at(rest, start_axis, ratios, depth + 1)?),
        }),
    }
}

fn collect_tree_leaves(tree: &DwindleTree, out: &mut Vec<WindowId>) {
    match tree {
        DwindleTree::Leaf(id) => out.push(*id),
        DwindleTree::Split { first, second, .. } => {
            collect_tree_leaves(first, out);
            collect_tree_leaves(second, out);
        }
    }
}

fn tree_contains(tree: &DwindleTree, id: WindowId) -> bool {
    match tree {
        DwindleTree::Leaf(leaf) => *leaf == id,
        DwindleTree::Split { first, second, .. } => {
            tree_contains(first, id) || tree_contains(second, id)
        }
    }
}

fn append_tiled_leaf(
    tree: Option<DwindleTree>,
    id: WindowId,
    start_axis: SplitAxis,
) -> DwindleTree {
    match tree {
        Some(tree) => append_tiled_leaf_at(tree, id, start_axis, 0),
        None => DwindleTree::Leaf(id),
    }
}

fn append_tiled_leaf_at(
    tree: DwindleTree,
    id: WindowId,
    start_axis: SplitAxis,
    depth: usize,
) -> DwindleTree {
    match tree {
        DwindleTree::Leaf(existing) => DwindleTree::Split {
            axis: start_axis.at_depth(depth),
            ratio: DEFAULT_RATIO,
            first: Box::new(DwindleTree::Leaf(existing)),
            second: Box::new(DwindleTree::Leaf(id)),
        },
        DwindleTree::Split {
            axis,
            ratio,
            first,
            second,
        } => DwindleTree::Split {
            axis,
            ratio,
            first,
            second: Box::new(append_tiled_leaf_at(*second, id, start_axis, depth + 1)),
        },
    }
}

fn prune_tree_to_ids(tree: DwindleTree, active_ids: &[WindowId]) -> Option<DwindleTree> {
    match tree {
        DwindleTree::Leaf(id) => active_ids.contains(&id).then_some(DwindleTree::Leaf(id)),
        DwindleTree::Split {
            axis,
            ratio,
            first,
            second,
        } => match (
            prune_tree_to_ids(*first, active_ids),
            prune_tree_to_ids(*second, active_ids),
        ) {
            (Some(first), Some(second)) => Some(DwindleTree::Split {
                axis,
                ratio,
                first: Box::new(first),
                second: Box::new(second),
            }),
            (Some(only), None) | (None, Some(only)) => Some(only),
            (None, None) => None,
        },
    }
}

fn remove_tree_leaf(tree: DwindleTree, id: WindowId) -> (Option<DwindleTree>, bool) {
    match tree {
        DwindleTree::Leaf(leaf) if leaf == id => (None, true),
        DwindleTree::Leaf(leaf) => (Some(DwindleTree::Leaf(leaf)), false),
        DwindleTree::Split {
            axis,
            ratio,
            first,
            second,
        } => {
            let (first, removed_first) = remove_tree_leaf(*first, id);
            let (second, removed_second) = remove_tree_leaf(*second, id);
            let removed = removed_first || removed_second;
            let tree = match (first, second) {
                (Some(first), Some(second)) => Some(DwindleTree::Split {
                    axis,
                    ratio,
                    first: Box::new(first),
                    second: Box::new(second),
                }),
                (Some(only), None) | (None, Some(only)) => Some(only),
                (None, None) => None,
            };
            (tree, removed)
        }
    }
}

fn insert_leaf_around_target(
    tree: DwindleTree,
    target: WindowId,
    moving: WindowId,
    axis: SplitAxis,
    moving_first: bool,
) -> Option<DwindleTree> {
    match tree {
        DwindleTree::Leaf(id) if id == target => {
            let moving = DwindleTree::Leaf(moving);
            let target = DwindleTree::Leaf(target);
            let (first, second) = if moving_first {
                (moving, target)
            } else {
                (target, moving)
            };
            Some(DwindleTree::Split {
                axis,
                ratio: 0.5,
                first: Box::new(first),
                second: Box::new(second),
            })
        }
        DwindleTree::Leaf(_) => None,
        DwindleTree::Split {
            axis: split_axis,
            ratio,
            first,
            second,
        } => {
            let first = *first;
            let second = *second;
            if tree_contains(&first, target) {
                insert_leaf_around_target(first, target, moving, axis, moving_first).map(
                    |inserted| DwindleTree::Split {
                        axis: split_axis,
                        ratio,
                        first: Box::new(inserted),
                        second: Box::new(second),
                    },
                )
            } else if tree_contains(&second, target) {
                insert_leaf_around_target(second, target, moving, axis, moving_first).map(
                    |inserted| DwindleTree::Split {
                        axis: split_axis,
                        ratio,
                        first: Box::new(first),
                        second: Box::new(inserted),
                    },
                )
            } else {
                None
            }
        }
    }
}

fn adjust_tree_split_for_focused(
    tree: &mut DwindleTree,
    focused: WindowId,
    delta: f32,
    depth: usize,
) -> Option<usize> {
    match tree {
        DwindleTree::Leaf(_) => None,
        DwindleTree::Split {
            ratio,
            first,
            second,
            ..
        } => {
            if tree_contains(first.as_ref(), focused) {
                if let Some(index) = adjust_tree_split_for_focused(first, focused, delta, depth + 1)
                {
                    return Some(index);
                }
                *ratio = adjust_ratio_value(*ratio, delta);
                Some(depth)
            } else if tree_contains(second.as_ref(), focused) {
                if let Some(index) =
                    adjust_tree_split_for_focused(second, focused, delta, depth + 1)
                {
                    return Some(index);
                }
                *ratio = adjust_ratio_value(*ratio, -delta);
                Some(depth)
            } else {
                None
            }
        }
    }
}

fn flip_tree_split_for_focused(
    tree: &mut DwindleTree,
    focused: WindowId,
    depth: usize,
) -> Option<(usize, SplitAxis)> {
    match tree {
        DwindleTree::Leaf(_) => None,
        DwindleTree::Split {
            axis,
            first,
            second,
            ..
        } => {
            if tree_contains(first.as_ref(), focused) {
                if let Some(result) = flip_tree_split_for_focused(first, focused, depth + 1) {
                    return Some(result);
                }
            } else if tree_contains(second.as_ref(), focused) {
                if let Some(result) = flip_tree_split_for_focused(second, focused, depth + 1) {
                    return Some(result);
                }
            } else {
                return None;
            }

            *axis = axis.flipped();
            Some((depth, *axis))
        }
    }
}

fn allocate_dwindle(
    tree: &DwindleTree,
    rect: FloatRect,
    gap: f32,
    placements: &mut Vec<WindowPlacement>,
) {
    match tree {
        DwindleTree::Leaf(id) => placements.push(WindowPlacement { id: *id, rect }),
        DwindleTree::Split {
            axis,
            ratio,
            first,
            second,
        } => {
            let (first_rect, second_rect) = split_float_rect(rect, *axis, *ratio, gap);
            allocate_dwindle(first, first_rect, gap, placements);
            allocate_dwindle(second, second_rect, gap, placements);
        }
    }
}

fn split_float_rect(
    rect: FloatRect,
    axis: SplitAxis,
    ratio: f32,
    gap: f32,
) -> (FloatRect, FloatRect) {
    let ratio = clamp_split_ratio(ratio);
    // Snap the divider to a whole cell so the two children stay flush. Each leaf rect is
    // rounded independently at render time (`FloatRect::to_rect`); if the boundary were
    // fractional, rounding the first child's size and the second child's origin
    // separately could drift by a cell, leaving an extra gap row/column between tiles.
    // With an integer-aligned container (the canvas bounds are whole cells) and an
    // integer gap, rounding `first` keeps `second` integer too: first + gap == second's
    // origin and the pair exactly fills the container.
    match axis {
        SplitAxis::Horizontal => {
            let gap = if rect.w > gap { gap } else { 0.0 };
            let available = (rect.w - gap).max(0.0);
            let first_w = (available * ratio).round();
            let second_w = available - first_w;
            (
                FloatRect {
                    x: rect.x,
                    y: rect.y,
                    w: first_w,
                    h: rect.h,
                },
                FloatRect {
                    x: rect.x + first_w + gap,
                    y: rect.y,
                    w: second_w,
                    h: rect.h,
                },
            )
        }
        SplitAxis::Vertical => {
            let gap = if rect.h > gap { gap } else { 0.0 };
            let available = (rect.h - gap).max(0.0);
            let first_h = (available * ratio).round();
            let second_h = available - first_h;
            (
                FloatRect {
                    x: rect.x,
                    y: rect.y,
                    w: rect.w,
                    h: first_h,
                },
                FloatRect {
                    x: rect.x,
                    y: rect.y + first_h + gap,
                    w: rect.w,
                    h: second_h,
                },
            )
        }
    }
}

fn ratio_at(ratios: &[f32], index: usize) -> f32 {
    ratios
        .get(index)
        .copied()
        .map(clamp_split_ratio)
        .unwrap_or(DEFAULT_RATIO)
}

fn clamp_split_ratio(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(MIN_SPLIT_RATIO, MAX_SPLIT_RATIO)
    } else {
        DEFAULT_RATIO
    }
}

fn adjust_ratio_value(value: f32, delta: f32) -> f32 {
    clamp_split_ratio(value + delta)
}

fn inset_float_rect(rect: FloatRect, inset: f32) -> FloatRect {
    let horizontal = inset * 2.0;
    let vertical = inset * 2.0;
    FloatRect {
        x: rect.x + inset.min(rect.w / 2.0),
        y: rect.y + inset.min(rect.h / 2.0),
        w: (rect.w - horizontal).max(1.0),
        h: (rect.h - vertical).max(1.0),
    }
}

/// Clamp a floating window's size into the usable range for `bounds`.
fn clamp_window_size(rect: FloatRect, bounds: FloatRect) -> (f32, f32) {
    let max_w = bounds.w.max(1.0);
    let max_h = bounds.h.max(1.0);
    let min_w = max_w.min(18.0);
    let min_h = max_h.min(6.0);
    (
        rect.w.max(1.0).clamp(min_w, max_w),
        rect.h.max(1.0).clamp(min_h, max_h),
    )
}

/// Clamp a window fully inside `bounds`. Used for tiled placement and for placing a
/// freshly floated window so it always starts on screen.
fn clamp_float_rect(rect: FloatRect, bounds: FloatRect) -> FloatRect {
    let (w, h) = clamp_window_size(rect, bounds);
    let max_x = (bounds.x + bounds.w - w).max(bounds.x);
    let max_y = (bounds.y + bounds.h - h).max(bounds.y);
    FloatRect {
        x: rect.x.clamp(bounds.x, max_x),
        y: rect.y.clamp(bounds.y, max_y),
        w,
        h,
    }
}

/// Clamp a floating window that may hang off the terminal edges. The window can extend
/// past `bounds` — the `Canvas` clips the overflow at render time, the same clipping the
/// drag-and-drop preview relies on — but at least [`OFFSCREEN_MIN_VISIBLE`] cells stay on
/// each axis so it can always be grabbed and dragged back into view.
fn clamp_floating_rect(rect: FloatRect, bounds: FloatRect) -> FloatRect {
    let (w, h) = clamp_window_size(rect, bounds);
    let margin_x = OFFSCREEN_MIN_VISIBLE.min(w);
    let margin_y = OFFSCREEN_MIN_VISIBLE.min(h);
    let lo_x = bounds.x + margin_x - w;
    let hi_x = bounds.x + bounds.w - margin_x;
    let lo_y = bounds.y + margin_y - h;
    let hi_y = bounds.y + bounds.h - margin_y;
    FloatRect {
        x: rect.x.clamp(lo_x.min(hi_x), hi_x.max(lo_x)),
        y: rect.y.clamp(lo_y.min(hi_y), hi_y.max(lo_y)),
        w,
        h,
    }
}

/// Float rect for a window leaving the tiling: keep its remembered floating size but
/// center it on the tile it currently occupies, so it lifts off in place instead of
/// jumping to a fixed default spot.
fn lift_off_float_rect(
    tile_rect: FloatRect,
    remembered: FloatRect,
    bounds: FloatRect,
) -> FloatRect {
    let w = remembered.w;
    let h = remembered.h;
    let center_x = tile_rect.x + tile_rect.w / 2.0;
    let center_y = tile_rect.y + tile_rect.h / 2.0;
    clamp_float_rect(
        FloatRect {
            x: center_x - w / 2.0,
            y: center_y - h / 2.0,
            w,
            h,
        },
        bounds,
    )
}

fn nearest_resize_corner(event: MouseDragEvent) -> ResizeCorner {
    nearest_resize_corner_from_local(
        event.from_local_x,
        event.from_local_y,
        event.target_w,
        event.target_h,
    )
}

fn nearest_resize_corner_from_local(
    local_x: u16,
    local_y: u16,
    target_w: u16,
    target_h: u16,
) -> ResizeCorner {
    let x = f32::from(local_x);
    let y = f32::from(local_y);
    let right = f32::from(target_w.saturating_sub(1));
    let bottom = f32::from(target_h.saturating_sub(1));
    let corners = [
        (ResizeCorner::UpperLeft, 0.0, 0.0),
        (ResizeCorner::UpperRight, right, 0.0),
        (ResizeCorner::LowerLeft, 0.0, bottom),
        (ResizeCorner::LowerRight, right, bottom),
    ];

    corners
        .into_iter()
        .min_by(|(_, ax, ay), (_, bx, by)| {
            let a = (x - ax).powi(2) + (y - ay).powi(2);
            let b = (x - bx).powi(2) + (y - by).powi(2);
            a.total_cmp(&b)
        })
        .map(|(corner, _, _)| corner)
        .unwrap_or(ResizeCorner::LowerRight)
}

fn resize_float_rect_from_corner(
    rect: FloatRect,
    corner: ResizeCorner,
    dx: f32,
    dy: f32,
    bounds: FloatRect,
) -> FloatRect {
    let rect = clamp_float_rect(rect, bounds);
    let max_w = bounds.w.max(1.0);
    let max_h = bounds.h.max(1.0);
    let min_w = max_w.min(18.0);
    let min_h = max_h.min(6.0);
    let mut left = rect.x;
    let mut right = rect.x + rect.w;
    let mut top = rect.y;
    let mut bottom = rect.y + rect.h;

    match corner {
        ResizeCorner::UpperLeft => {
            left += dx;
            top += dy;
        }
        ResizeCorner::UpperRight => {
            right += dx;
            top += dy;
        }
        ResizeCorner::LowerLeft => {
            left += dx;
            bottom += dy;
        }
        ResizeCorner::LowerRight => {
            right += dx;
            bottom += dy;
        }
    }

    match corner {
        ResizeCorner::UpperLeft | ResizeCorner::LowerLeft => {
            left = left.clamp(bounds.x, right - min_w);
        }
        ResizeCorner::UpperRight | ResizeCorner::LowerRight => {
            right = right.clamp(left + min_w, bounds.x + bounds.w);
        }
    }
    match corner {
        ResizeCorner::UpperLeft | ResizeCorner::UpperRight => {
            top = top.clamp(bounds.y, bottom - min_h);
        }
        ResizeCorner::LowerLeft | ResizeCorner::LowerRight => {
            bottom = bottom.clamp(top + min_h, bounds.y + bounds.h);
        }
    }

    FloatRect {
        x: left,
        y: top,
        w: (right - left).max(1.0),
        h: (bottom - top).max(1.0),
    }
}

/// Closing windows shrink only slightly toward their center while the opacity fade
/// (see `window_element`) carries the dismissal, instead of collapsing to a tiny box.
fn close_rect(rect: FloatRect) -> FloatRect {
    const SCALE: f32 = 0.9;
    let w = (rect.w * SCALE).max(1.0);
    let h = (rect.h * SCALE).max(1.0);
    FloatRect {
        x: rect.x + (rect.w - w) / 2.0,
        y: rect.y + (rect.h - h) / 2.0,
        w,
        h,
    }
}

fn default_floating_rect(bounds: FloatRect, seed: WindowId) -> FloatRect {
    let w = (bounds.w * 0.42).clamp(bounds.w.min(24.0), bounds.w.max(1.0));
    let h = (bounds.h * 0.42).clamp(bounds.h.min(8.0), bounds.h.max(1.0));
    let offset = (seed % 7) as f32 * 3.0;
    clamp_float_rect(
        FloatRect {
            x: bounds.x + 3.0 + offset,
            y: bounds.y + 2.0 + offset / 2.0,
            w,
            h,
        },
        bounds,
    )
}

fn tiled_drag_preview_rect(
    tile_rect: FloatRect,
    remembered_float_rect: FloatRect,
    bounds: FloatRect,
    from_local_x: u16,
    from_local_y: u16,
    target_w: u16,
    target_h: u16,
) -> FloatRect {
    let remembered = clamp_float_rect(remembered_float_rect, bounds);
    let anchor_x = if target_w == 0 {
        0.5
    } else {
        (f32::from(from_local_x) / f32::from(target_w)).clamp(0.0, 1.0)
    };
    let anchor_y = if target_h == 0 {
        0.5
    } else {
        (f32::from(from_local_y) / f32::from(target_h)).clamp(0.0, 1.0)
    };

    clamp_float_rect(
        FloatRect {
            x: tile_rect.x + f32::from(from_local_x) - remembered.w * anchor_x,
            y: tile_rect.y + f32::from(from_local_y) - remembered.h * anchor_y,
            w: remembered.w,
            h: remembered.h,
        },
        bounds,
    )
}

fn spawn_window(ctx: &mut Context<WindowManagerDemo>, animations: WindowAnimationConfig) -> Update {
    let bounds = canvas_bounds_from_viewport(ctx.viewport());
    let id = ctx.state.next_window_id;
    ctx.state.next_window_id = ctx.state.next_window_id.saturating_add(1);
    let app = DemoApp::for_index(id as usize);
    let floating_rect = default_floating_rect(bounds, id);

    let window = WindowState {
        id,
        title: app.title().to_string(),
        app,
        floating: false,
        fullscreen: false,
        floating_rect,
        opening: true,
        closing: false,
    };

    let workspace = &mut ctx.state.workspaces[ctx.state.active_workspace];
    let previous_focused = workspace.focused_window;
    workspace.windows.push(window);
    let placement = place_spawned_window(workspace, id, previous_focused, bounds);
    workspace.focused_window = Some(id);

    ctx.state.focused_window = Some(id);
    request_window_focus(ctx, id);
    // Surrounding windows animate to make room; the new one appears at its slot.
    ctx.state.animation = GeometryAnimation::Spawn;
    ctx.state.status = match placement {
        SpawnPlacement::Split(target) => {
            format!("Spawned {} #{id} split off focused #{target}", app.title())
        }
        SpawnPlacement::Appended => format!("Spawned {} as tiled window #{id}", app.title()),
    };
    Update::with_command(finish_open_command(id, open_delay(animations)))
}

/// Outcome of placing a freshly spawned window, for status reporting.
enum SpawnPlacement {
    /// Split off the focused tile (the given window).
    Split(WindowId),
    /// Appended to the tree (first tiled window, or no valid split target).
    Appended,
}

/// Insert `id` by splitting the focused window — Hyprland's dwindle behavior: a new
/// window always splits the currently focused one, never the tile under the cursor.
/// Falls back to a plain append when there is no valid split target (the first window,
/// or a floating focus). The split *axis* comes from the focused tile's shape
/// (`spawn_split_for_rect`).
fn place_spawned_window(
    workspace: &mut Workspace,
    id: WindowId,
    previous_focused: Option<WindowId>,
    bounds: FloatRect,
) -> SpawnPlacement {
    if let Some(target) = previous_focused.filter(|target| *target != id) {
        let placements = workspace_target_rects_excluding(workspace, bounds, Some(id));
        if let Some(rect) = placement_for(&placements, target) {
            let (axis, moving_first) = spawn_split_for_rect(rect);
            if insert_tiled_window_around_target(workspace, id, target, axis, moving_first) {
                return SpawnPlacement::Split(target);
            }
        }
    }

    append_tiled_window(workspace, id);
    SpawnPlacement::Appended
}

/// Dwindle split direction for the focused tile: split the longer side so the two halves
/// stay roughly square — Hyprland compares the node's width vs height (wider → split
/// side-by-side, taller → split top/bottom). Terminal cells are about twice as tall as
/// wide, so width is weighted by [`SPLIT_WIDTH_MULTIPLIER`] (Hyprland's
/// `split_width_multiplier`) before the comparison. The new window takes the second
/// (right/bottom) slot — a fixed side, not the cursor (Hyprland's `force_split = 2`).
fn spawn_split_for_rect(rect: FloatRect) -> (SplitAxis, bool) {
    let axis = if rect.w >= rect.h * SPLIT_WIDTH_MULTIPLIER {
        SplitAxis::Horizontal
    } else {
        SplitAxis::Vertical
    };
    (axis, false)
}

fn close_focused_window(
    ctx: &mut Context<WindowManagerDemo>,
    animations: WindowAnimationConfig,
) -> Update {
    let Some(id) = ctx.state.focused_window else {
        ctx.state.status = "No focused window to close".to_string();
        return Update::full();
    };

    let bounds = canvas_bounds_from_viewport(ctx.viewport());
    let placements = {
        let workspace = &ctx.state.workspaces[ctx.state.active_workspace];
        workspace_target_rects(workspace, bounds)
    };
    let mut closed = false;
    if let Some(window) = active_window_mut(&mut ctx.state, id)
        && !window.closing
    {
        window.floating_rect = placement_for(&placements, id).unwrap_or(window.floating_rect);
        window.opening = false;
        window.closing = true;
        closed = true;
    }

    if closed {
        ctx.state.animation = GeometryAnimation::Close;
        choose_fallback_focus(&mut ctx.state);
        request_current_window_focus(ctx);
        ctx.state.status = format!("Closing window #{id}");
        Update::with_command(prune_closed_command(id, close_delay(animations)))
    } else {
        Update::full()
    }
}

fn toggle_tiling(ctx: &mut Context<WindowManagerDemo>) {
    let Some(id) = ctx.state.focused_window else {
        ctx.state.status = "No focused window to toggle".to_string();
        return;
    };

    let bounds = canvas_bounds_from_viewport(ctx.viewport());

    // The window's current on-screen placement, captured before the tree changes, so a
    // tiled window lifts off over the spot it occupied instead of a fixed default.
    let current_rect = {
        let workspace = &ctx.state.workspaces[ctx.state.active_workspace];
        placement_for(&workspace_target_rects(workspace, bounds), id)
    };

    let mut insert_tiled_at = None;
    let mut remove_from_tiling = false;
    if let Some(window) = active_window_mut(&mut ctx.state, id) {
        window.opening = false;
        window.fullscreen = false;
        if window.floating {
            window.floating_rect = clamp_float_rect(window.floating_rect, bounds);
            insert_tiled_at = Some(rect_center(window.floating_rect));
            window.floating = false;
            ctx.state.animation = GeometryAnimation::TileFloat;
            ctx.state.status = format!("Window #{id} returned to dwindle tiling");
        } else {
            // Lift off centered on the tile (keeping the remembered float size) rather
            // than snapping to the remembered/default floating position.
            window.floating_rect = match current_rect {
                Some(tile) => lift_off_float_rect(tile, window.floating_rect, bounds),
                None => clamp_float_rect(window.floating_rect, bounds),
            };
            window.floating = true;
            remove_from_tiling = true;
            ctx.state.animation = GeometryAnimation::TileFloat;
            ctx.state.status = format!("Window #{id} is now floating; drag to move");
        }
    }

    let mut status_after_tree_update = None;
    if insert_tiled_at.is_some() || remove_from_tiling {
        let workspace = &mut ctx.state.workspaces[ctx.state.active_workspace];
        if let Some(point) = insert_tiled_at {
            if let Some((target_id, moving_first)) =
                insert_tiled_window_at_point(workspace, id, point, bounds)
            {
                status_after_tree_update = Some(format!(
                    "Window #{id} returned to tiling {} #{target_id}",
                    if moving_first { "before" } else { "after" }
                ));
            } else {
                append_tiled_window(workspace, id);
                status_after_tree_update = Some(format!("Window #{id} returned to dwindle tiling"));
            }
        } else if remove_from_tiling {
            remove_tiled_window(workspace, id);
        }
    }
    if let Some(status) = status_after_tree_update {
        ctx.state.status = status;
    }
    request_window_focus(ctx, id);
}

fn toggle_fullscreen(ctx: &mut Context<WindowManagerDemo>) -> Update {
    let Some(id) = ctx.state.focused_window else {
        ctx.state.status = "No focused window to fullscreen".to_string();
        return Update::full();
    };

    let bounds = canvas_bounds_from_viewport(ctx.viewport());
    let placements = {
        let workspace = &ctx.state.workspaces[ctx.state.active_workspace];
        workspace_target_rects(workspace, bounds)
    };

    let mut status = None;
    if let Some(window) = active_window_mut(&mut ctx.state, id) {
        window.opening = false;
        if !window.fullscreen && window.floating {
            window.floating_rect = placement_for(&placements, id).unwrap_or(window.floating_rect);
        }
        window.fullscreen = !window.fullscreen;
        status = Some(if window.fullscreen {
            format!("Window #{id} is fullscreen")
        } else {
            format!("Window #{id} left fullscreen")
        });
    }

    if let Some(status) = status {
        ctx.state.animation = GeometryAnimation::Fullscreen;
        request_window_focus(ctx, id);
        ctx.state.status = status;
    }

    Update::full()
}

fn toggle_focused_split_axis(state: &mut State) {
    let Some(focused) = state.focused_window else {
        state.status = "No focused tiled window for split toggle".to_string();
        return;
    };
    let workspace = &mut state.workspaces[state.active_workspace];
    if !workspace
        .active_tiled_ids_by_window_order()
        .contains(&focused)
    {
        state.status = "Focused window is floating; no split axis to toggle".to_string();
        return;
    }
    workspace.tile_tree = effective_tile_tree(workspace, None);
    let Some(tree) = workspace.tile_tree.as_mut() else {
        state.status = "Focused tiled window is alone; no split axis to toggle".to_string();
        return;
    };
    let Some((depth, axis)) = flip_tree_split_for_focused(tree, focused, 0) else {
        state.status = "Focused tiled window is alone; no split axis to toggle".to_string();
        return;
    };

    state.animation = GeometryAnimation::AxisChange;
    state.status = format!("Flipped focused split {} to {}", depth + 1, axis.label());
}

fn adjust_focused_split_ratio(state: &mut State, delta: f32) {
    let Some(focused) = state.focused_window else {
        state.status = "No focused tiled window for split adjustment".to_string();
        return;
    };
    let workspace = &mut state.workspaces[state.active_workspace];
    if workspace.tile_tree.is_none() {
        workspace.tile_tree = effective_tile_tree(workspace, None);
    }
    let Some(tree) = workspace.tile_tree.as_mut() else {
        state.status = "Focused window is floating or alone; no split ratio to adjust".to_string();
        return;
    };
    let Some(index) = adjust_tree_split_for_focused(tree, focused, delta, 0) else {
        state.status = "Focused window is floating or alone; no split ratio to adjust".to_string();
        return;
    };

    state.animation = GeometryAnimation::None;
    state.status = format!("Adjusted split {} by {:+.0}%", index + 1, delta * 100.0);
}

fn resize_window(
    state: &mut State,
    id: WindowId,
    corner: ResizeCorner,
    dx: i16,
    dy: i16,
    viewport: Rect,
) {
    focus_window(state, id);
    let bounds = canvas_bounds_from_viewport(viewport);

    let Some(window) = active_window_mut(state, id) else {
        state.status = "Window to resize was not found".to_string();
        return;
    };

    if window.fullscreen {
        state.status = format!("Window #{id} is fullscreen; press Alt+F before resizing");
        return;
    }

    if window.floating {
        window.floating_rect = resize_float_rect_from_corner(
            window.floating_rect,
            corner,
            f32::from(dx),
            f32::from(dy),
            bounds,
        );
        state.status = format!(
            "Resized floating #{id} from {} by {dx:+}, {dy:+}",
            corner.label()
        );
        return;
    }

    // Match the cursor like floating resize: move the nearest split of each axis by
    // exactly the mouse delta along that axis. A split's divider sits at
    // `ratio * available`, so `ratio_delta = pixels / available` moves it `pixels`
    // cells. The old code summed both axes and scaled by a fixed `0.01`, which
    // overshot on the wide (horizontal) axis and barely moved on the short one.
    let effective_dx = match corner {
        ResizeCorner::UpperLeft | ResizeCorner::LowerLeft => -dx,
        ResizeCorner::UpperRight | ResizeCorner::LowerRight => dx,
    };
    let effective_dy = match corner {
        ResizeCorner::UpperLeft | ResizeCorner::UpperRight => -dy,
        ResizeCorner::LowerLeft | ResizeCorner::LowerRight => dy,
    };

    let tile_bounds = inset_float_rect(bounds, OUTER_GAP);
    let Some(tree) = effective_tile_tree(&state.workspaces[state.active_workspace], None) else {
        state.status = format!("Window #{id} is alone; no split to resize");
        return;
    };

    // The grabbed corner's edge on each axis. An edge sitting on the terminal boundary
    // has no divider to drag, so resizing along that axis would otherwise move the
    // window's *inner* divider in an inverted way — block it instead.
    let focused_rect = {
        let mut placements = Vec::new();
        allocate_dwindle(&tree, tile_bounds, TILE_GAP, &mut placements);
        placement_for(&placements, id)
    };
    let grabbed_left = matches!(corner, ResizeCorner::UpperLeft | ResizeCorner::LowerLeft);
    let grabbed_top = matches!(corner, ResizeCorner::UpperLeft | ResizeCorner::UpperRight);
    let grabbed_edge_on_outer_border = |axis: SplitAxis| -> bool {
        const EDGE_EPS: f32 = 0.5;
        let Some(r) = focused_rect else {
            return false;
        };
        match axis {
            SplitAxis::Horizontal if grabbed_left => r.x <= tile_bounds.x + EDGE_EPS,
            SplitAxis::Horizontal => r.x + r.w >= tile_bounds.x + tile_bounds.w - EDGE_EPS,
            SplitAxis::Vertical if grabbed_top => r.y <= tile_bounds.y + EDGE_EPS,
            SplitAxis::Vertical => r.y + r.h >= tile_bounds.y + tile_bounds.h - EDGE_EPS,
        }
    };

    // Container extents are read from the pre-adjustment tree. Horizontal and vertical
    // splits divide orthogonal extents, so adjusting one never invalidates the other's.
    let mut adjusted = false;
    for (axis, pixels) in [
        (SplitAxis::Horizontal, f32::from(effective_dx)),
        (SplitAxis::Vertical, f32::from(effective_dy)),
    ] {
        if pixels == 0.0 || grabbed_edge_on_outer_border(axis) {
            continue;
        }
        if let Some(available) = nearest_split_available(&tree, tile_bounds, TILE_GAP, id, axis) {
            adjusted |= resize_tiled_split(state, id, axis, available, pixels);
        }
    }

    state.animation = GeometryAnimation::None;
    state.status = if adjusted {
        format!("Resized tiled #{id} from {}", corner.label())
    } else {
        format!("Window #{id} has no divider to resize on that axis")
    };
}

/// Available extent (container size minus gap) of the deepest split of `target_axis`
/// on the path to `focused`, mirroring `split_float_rect`'s gap handling. `None` when
/// no such split exists (e.g. the window has no divider along that axis).
fn nearest_split_available(
    tree: &DwindleTree,
    rect: FloatRect,
    gap: f32,
    focused: WindowId,
    target_axis: SplitAxis,
) -> Option<f32> {
    let DwindleTree::Split {
        axis,
        ratio,
        first,
        second,
    } = tree
    else {
        return None;
    };

    let (first_rect, second_rect) = split_float_rect(rect, *axis, *ratio, gap);
    let (child, child_rect) = if tree_contains(first, focused) {
        (first.as_ref(), first_rect)
    } else if tree_contains(second, focused) {
        (second.as_ref(), second_rect)
    } else {
        return None;
    };

    // Prefer a deeper split of the same axis (nearest the focused leaf).
    if let Some(deeper) = nearest_split_available(child, child_rect, gap, focused, target_axis) {
        return Some(deeper);
    }

    if *axis != target_axis {
        return None;
    }
    let extent = match target_axis {
        SplitAxis::Horizontal => rect.w,
        SplitAxis::Vertical => rect.h,
    };
    let usable_gap = if extent > gap { gap } else { 0.0 };
    Some((extent - usable_gap).max(1.0))
}

/// Adjust the deepest split of `target_axis` on the path to `focused` by
/// `pixels / available`, initializing the persisted tree the same way
/// `adjust_focused_split_ratio` does. Returns whether a split was adjusted.
fn resize_tiled_split(
    state: &mut State,
    focused: WindowId,
    target_axis: SplitAxis,
    available: f32,
    pixels: f32,
) -> bool {
    let workspace = &mut state.workspaces[state.active_workspace];
    if workspace.tile_tree.is_none() {
        workspace.tile_tree = effective_tile_tree(workspace, None);
    }
    let Some(tree) = workspace.tile_tree.as_mut() else {
        return false;
    };
    let ratio_delta = pixels / available.max(1.0);
    adjust_nearest_axis_split(tree, focused, target_axis, ratio_delta)
}

/// Mutate the deepest split of `target_axis` on the path to `focused`. The ratio grows
/// when `focused` is in the first child and shrinks when it is in the second, matching
/// `adjust_tree_split_for_focused`'s sign convention.
fn adjust_nearest_axis_split(
    tree: &mut DwindleTree,
    focused: WindowId,
    target_axis: SplitAxis,
    delta: f32,
) -> bool {
    let DwindleTree::Split {
        axis,
        ratio,
        first,
        second,
    } = tree
    else {
        return false;
    };

    if tree_contains(first, focused) {
        if adjust_nearest_axis_split(first, focused, target_axis, delta) {
            return true;
        }
        if *axis == target_axis {
            *ratio = adjust_ratio_value(*ratio, delta);
            return true;
        }
        false
    } else if tree_contains(second, focused) {
        if adjust_nearest_axis_split(second, focused, target_axis, delta) {
            return true;
        }
        if *axis == target_axis {
            *ratio = adjust_ratio_value(*ratio, -delta);
            return true;
        }
        false
    } else {
        false
    }
}

fn drop_tiled_window_at(state: &mut State, id: WindowId, x: u16, y: u16, viewport: Rect) {
    state.animation = GeometryAnimation::TileFloat;
    let bounds = canvas_bounds_from_viewport(viewport);
    let drop_point = canvas_local_point_from_mouse(x, y, bounds);
    let target = {
        let workspace = &state.workspaces[state.active_workspace];
        let placements = workspace_target_rects_excluding(workspace, bounds, Some(id));
        let tiled_ids: Vec<WindowId> = workspace
            .tiled_ids()
            .into_iter()
            .filter(|target_id| *target_id != id)
            .collect();
        target_tiled_window_for_drop(&placements, &tiled_ids, drop_point).and_then(|target_id| {
            placement_for(&placements, target_id).map(|rect| (target_id, rect))
        })
    };

    let Some((target_id, target_rect)) = target else {
        state.status = format!("Tiled window #{id} stayed in place; no target tile");
        return;
    };

    let (axis, moving_first) = drop_split_for_target(target_rect, drop_point);
    let workspace = &mut state.workspaces[state.active_workspace];
    if move_tiled_window_around_target(workspace, id, target_id, axis, moving_first) {
        state.status = format!(
            "Inserted tiled window #{id} {} #{target_id}",
            if moving_first { "before" } else { "after" }
        );
    } else {
        state.status = format!("Tiled window #{id} stayed in place");
    }
}

fn canvas_local_point_from_mouse(x: u16, y: u16, bounds: FloatRect) -> (f32, f32) {
    (
        f32::from(x).clamp(bounds.x, bounds.x + bounds.w),
        f32::from(y.saturating_sub(TOP_BAR_HEIGHT)).clamp(bounds.y, bounds.y + bounds.h),
    )
}

fn target_tiled_window_for_drop(
    placements: &[WindowPlacement],
    tiled_ids: &[WindowId],
    point: (f32, f32),
) -> Option<WindowId> {
    placements
        .iter()
        .rev()
        .find(|placement| float_rect_contains_point(placement.rect, point))
        .and_then(|placement| tiled_ids.contains(&placement.id).then_some(placement.id))
}

fn drop_split_for_target(target_rect: FloatRect, point: (f32, f32)) -> (SplitAxis, bool) {
    let local_x = ((point.0 - target_rect.x) / target_rect.w.max(1.0)).clamp(0.0, 1.0);
    let local_y = ((point.1 - target_rect.y) / target_rect.h.max(1.0)).clamp(0.0, 1.0);
    let from_center_x = local_x - 0.5;
    let from_center_y = local_y - 0.5;

    if from_center_x.abs() >= from_center_y.abs() {
        (SplitAxis::Horizontal, from_center_x < 0.0)
    } else {
        (SplitAxis::Vertical, from_center_y < 0.0)
    }
}

fn insert_tiled_window_at_point(
    workspace: &mut Workspace,
    id: WindowId,
    point: (f32, f32),
    bounds: FloatRect,
) -> Option<(WindowId, bool)> {
    let target = {
        let placements = workspace_target_rects_excluding(workspace, bounds, Some(id));
        let tiled_ids: Vec<WindowId> = workspace
            .tiled_ids()
            .into_iter()
            .filter(|target_id| *target_id != id)
            .collect();
        target_tiled_window_for_drop(&placements, &tiled_ids, point).and_then(|target_id| {
            placement_for(&placements, target_id).map(|rect| (target_id, rect))
        })
    };

    let (target_id, target_rect) = target?;
    let (axis, moving_first) = drop_split_for_target(target_rect, point);
    insert_tiled_window_around_target(workspace, id, target_id, axis, moving_first)
        .then_some((target_id, moving_first))
}

fn insert_tiled_window_around_target(
    workspace: &mut Workspace,
    id: WindowId,
    target: WindowId,
    axis: SplitAxis,
    moving_first: bool,
) -> bool {
    if id == target {
        return false;
    }
    let Some(tree) = effective_tile_tree(workspace, Some(id)) else {
        return false;
    };
    let Some(inserted) = insert_leaf_around_target(tree, target, id, axis, moving_first) else {
        return false;
    };
    workspace.tile_tree = Some(inserted);
    true
}

fn move_tiled_window_around_target(
    workspace: &mut Workspace,
    moving: WindowId,
    target: WindowId,
    axis: SplitAxis,
    moving_first: bool,
) -> bool {
    if moving == target {
        return false;
    }
    if workspace.tile_tree.is_none() {
        workspace.tile_tree = effective_tile_tree(workspace, None);
    }
    let Some(tree) = workspace.tile_tree.take() else {
        return false;
    };
    let original = tree.clone();
    let (Some(without_moving), true) = remove_tree_leaf(tree, moving) else {
        workspace.tile_tree = Some(original);
        return false;
    };
    let Some(inserted) =
        insert_leaf_around_target(without_moving.clone(), target, moving, axis, moving_first)
    else {
        workspace.tile_tree = Some(original);
        return false;
    };
    workspace.tile_tree = Some(inserted);
    true
}

fn float_rect_contains_point(rect: FloatRect, point: (f32, f32)) -> bool {
    point.0 >= rect.x && point.0 < rect.x + rect.w && point.1 >= rect.y && point.1 < rect.y + rect.h
}

fn focus_in_direction(state: &mut State, direction: Direction, viewport: Rect) -> Option<WindowId> {
    let bounds = canvas_bounds_from_viewport(viewport);
    let workspace = &state.workspaces[state.active_workspace];
    let placements = workspace_target_rects(workspace, bounds);
    let candidates: Vec<WindowPlacement> = workspace
        .windows
        .iter()
        .filter(|window| !window.closing)
        .filter_map(|window| {
            placement_for(&placements, window.id).map(|rect| WindowPlacement {
                id: window.id,
                rect,
            })
        })
        .collect();

    if candidates.is_empty() {
        state.status = "No windows to focus".to_string();
        state.focused_window = None;
        return None;
    }

    let focused = state.focused_window.unwrap_or(candidates[0].id);
    let Some(current) = candidates.iter().find(|candidate| candidate.id == focused) else {
        let id = candidates[0].id;
        focus_window(state, id);
        return Some(id);
    };
    let next = candidates
        .iter()
        .filter(|candidate| candidate.id != focused)
        .filter_map(|candidate| {
            directional_score(current.rect, candidate.rect, direction)
                .map(|score| (candidate.id, score))
        })
        .min_by(|(_, a), (_, b)| a.total_cmp(b))
        .map(|(id, _)| id)
        .or_else(|| cycle_focus_id(&candidates, focused, direction));

    if let Some(next_id) = next {
        focus_window(state, next_id);
        state.status = format!("Spatial focus {:?}: #{} → #{}", direction, focused, next_id);
        Some(next_id)
    } else {
        None
    }
}

fn rect_center(rect: FloatRect) -> (f32, f32) {
    (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
}

fn directional_score(
    current: FloatRect,
    candidate: FloatRect,
    direction: Direction,
) -> Option<f32> {
    let current_center = rect_center(current);
    let candidate_center = rect_center(candidate);
    let current_right = current.x + current.w;
    let current_bottom = current.y + current.h;
    let candidate_right = candidate.x + candidate.w;
    let candidate_bottom = candidate.y + candidate.h;

    let (primary_gap, cross_overlap, cross_gap, center_offset) = match direction {
        Direction::Left => {
            if candidate_center.0 >= current_center.0 && candidate_right > current.x {
                return None;
            }
            (
                (current.x - candidate_right).max(0.0),
                interval_overlap(current.y, current_bottom, candidate.y, candidate_bottom),
                interval_gap(current.y, current_bottom, candidate.y, candidate_bottom),
                (candidate_center.1 - current_center.1).abs(),
            )
        }
        Direction::Right => {
            if candidate_center.0 <= current_center.0 && candidate.x < current_right {
                return None;
            }
            (
                (candidate.x - current_right).max(0.0),
                interval_overlap(current.y, current_bottom, candidate.y, candidate_bottom),
                interval_gap(current.y, current_bottom, candidate.y, candidate_bottom),
                (candidate_center.1 - current_center.1).abs(),
            )
        }
        Direction::Up => {
            if candidate_center.1 >= current_center.1 && candidate_bottom > current.y {
                return None;
            }
            (
                (current.y - candidate_bottom).max(0.0),
                interval_overlap(current.x, current_right, candidate.x, candidate_right),
                interval_gap(current.x, current_right, candidate.x, candidate_right),
                (candidate_center.0 - current_center.0).abs(),
            )
        }
        Direction::Down => {
            if candidate_center.1 <= current_center.1 && candidate.y < current_bottom {
                return None;
            }
            (
                (candidate.y - current_bottom).max(0.0),
                interval_overlap(current.x, current_right, candidate.x, candidate_right),
                interval_gap(current.x, current_right, candidate.x, candidate_right),
                (candidate_center.0 - current_center.0).abs(),
            )
        }
    };

    let overlap_penalty = if cross_overlap > 0.0 {
        0.0
    } else {
        10_000.0 + cross_gap * 100.0
    };

    Some(overlap_penalty + primary_gap * 10.0 + center_offset)
}

fn interval_overlap(a_start: f32, a_end: f32, b_start: f32, b_end: f32) -> f32 {
    (a_end.min(b_end) - a_start.max(b_start)).max(0.0)
}

fn interval_gap(a_start: f32, a_end: f32, b_start: f32, b_end: f32) -> f32 {
    if a_end < b_start {
        b_start - a_end
    } else if b_end < a_start {
        a_start - b_end
    } else {
        0.0
    }
}

fn cycle_focus_id(
    candidates: &[WindowPlacement],
    focused: WindowId,
    direction: Direction,
) -> Option<WindowId> {
    let index = candidates
        .iter()
        .position(|candidate| candidate.id == focused)
        .unwrap_or(0);
    let next_index = match direction {
        Direction::Left | Direction::Up => index
            .checked_sub(1)
            .unwrap_or_else(|| candidates.len().saturating_sub(1)),
        Direction::Right | Direction::Down => (index + 1) % candidates.len(),
    };
    candidates.get(next_index).map(|candidate| candidate.id)
}

fn switch_workspace(state: &mut State, index: usize) {
    if index >= state.workspaces.len() {
        return;
    }
    state.active_workspace = index;
    state.animation = GeometryAnimation::None;
    choose_fallback_focus(state);
    state.status = format!("Switched to workspace {}", index + 1);
}

fn move_focused_to_workspace(state: &mut State, target_index: usize) {
    if target_index >= state.workspaces.len() {
        return;
    }
    let source_index = state.active_workspace;
    let Some(focused) = state.focused_window else {
        state.status = "No focused window to move".to_string();
        return;
    };
    if source_index == target_index {
        state.status = format!(
            "Window #{focused} is already on workspace {}",
            target_index + 1
        );
        return;
    }

    let Some(position) = state.workspaces[source_index]
        .windows
        .iter()
        .position(|window| window.id == focused)
    else {
        state.status = "Focused window was not found".to_string();
        choose_fallback_focus(state);
        return;
    };

    let mut window = state.workspaces[source_index].windows.remove(position);
    if !window.floating {
        remove_tiled_window(&mut state.workspaces[source_index], window.id);
    }
    window.opening = false;
    window.closing = false;
    state.workspaces[target_index].focused_window = Some(window.id);
    if !window.floating {
        append_tiled_window(&mut state.workspaces[target_index], window.id);
    }
    state.workspaces[target_index].windows.push(window);
    state.animation = GeometryAnimation::None;
    choose_fallback_focus(state);
    state.status = format!("Moved window #{focused} to workspace {}", target_index + 1);
}

fn focus_window(state: &mut State, id: WindowId) {
    if state.workspaces[state.active_workspace]
        .windows
        .iter()
        .any(|window| window.id == id && !window.closing)
    {
        state.focused_window = Some(id);
        state.workspaces[state.active_workspace].focused_window = Some(id);
    }
}

fn choose_fallback_focus(state: &mut State) {
    let workspace = &mut state.workspaces[state.active_workspace];
    let focus = workspace
        .focused_window
        .filter(|focused| {
            workspace
                .windows
                .iter()
                .any(|window| window.id == *focused && !window.closing)
        })
        .or_else(|| {
            workspace
                .windows
                .iter()
                .find(|window| !window.closing)
                .map(|window| window.id)
        });
    workspace.focused_window = focus;
    state.focused_window = focus;
}

fn active_window_mut(state: &mut State, id: WindowId) -> Option<&mut WindowState> {
    state.workspaces[state.active_workspace]
        .windows
        .iter_mut()
        .find(|window| window.id == id)
}

fn find_window_mut(state: &mut State, id: WindowId) -> Option<&mut WindowState> {
    state
        .workspaces
        .iter_mut()
        .flat_map(|workspace| workspace.windows.iter_mut())
        .find(|window| window.id == id)
}

fn remove_window(state: &mut State, id: WindowId) {
    // Abandon any in-flight move/resize that targets the window being removed. Its
    // MouseRegion node disappears with it, so the framework stops emitting drag
    // events and `EndMove`/`EndResize` would never fire to clear these otherwise.
    if state.moving_window.is_some_and(|session| session.id == id) {
        state.moving_window = None;
    }
    if state
        .resizing_window
        .is_some_and(|session| session.id == id)
    {
        state.resizing_window = None;
    }

    for workspace in &mut state.workspaces {
        remove_tiled_window(workspace, id);
        workspace.windows.retain(|window| window.id != id);
        if workspace.focused_window == Some(id) {
            workspace.focused_window = workspace
                .windows
                .iter()
                .find(|window| !window.closing)
                .map(|window| window.id);
        }
    }
    if state.focused_window == Some(id) {
        choose_fallback_focus(state);
    }
}

fn open_delay(animations: WindowAnimationConfig) -> Duration {
    if animations.enabled && animations.spawn {
        animations.open_delay
    } else {
        Duration::ZERO
    }
}

fn close_delay(animations: WindowAnimationConfig) -> Duration {
    if animations.enabled && animations.close {
        animations.close_duration.max(animations.geometry_duration) + Duration::from_millis(20)
    } else {
        Duration::ZERO
    }
}

fn finish_open_command(id: WindowId, delay: Duration) -> Command {
    Command::spawn(move |link| {
        std::thread::sleep(delay);
        link.send(Msg::FinishOpen(id));
    })
}

fn prune_closed_command(id: WindowId, delay: Duration) -> Command {
    Command::spawn(move |link| {
        std::thread::sleep(delay);
        link.send(Msg::PruneClosed(id));
    })
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Canvas Window Manager")
        .mount(WindowManagerDemo::default())
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;

    use tui_lipan::TestBackend;

    const EPSILON: f32 = 0.001;

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < EPSILON,
            "expected {actual} to be close to {expected}"
        );
    }

    fn placement(placements: &[WindowPlacement], id: WindowId) -> FloatRect {
        placements
            .iter()
            .find(|placement| placement.id == id)
            .map(|placement| placement.rect)
            .expect("missing placement")
    }

    fn state_with_workspace(workspace: Workspace, next_window_id: WindowId) -> State {
        State {
            focused_window: workspace.focused_window,
            workspaces: vec![workspace],
            active_workspace: 0,
            next_window_id,
            status: String::new(),
            moving_window: None,
            resizing_window: None,
            animation: GeometryAnimation::None,
            last_viewport: Cell::new(None),
        }
    }

    #[test]
    fn ordered_windows_draws_tiled_closing_windows_under_expanding_windows() {
        let mut workspace = Workspace::new(0);
        let mut next = 1;
        seed_window(
            &mut workspace,
            &mut next,
            DemoApp::Terminal,
            false,
            FloatRect::default(),
        );
        seed_window(
            &mut workspace,
            &mut next,
            DemoApp::Files,
            false,
            FloatRect::default(),
        );
        seed_window(
            &mut workspace,
            &mut next,
            DemoApp::Chat,
            true,
            FloatRect::default(),
        );
        seed_window(
            &mut workspace,
            &mut next,
            DemoApp::Editor,
            true,
            FloatRect::default(),
        );
        workspace.windows[0].closing = true;
        workspace.windows[3].closing = true;

        let ids: Vec<WindowId> = ordered_windows(&workspace, Some(2))
            .into_iter()
            .map(|window| window.id)
            .collect();

        assert_eq!(ids, vec![1, 2, 3, 4]);
    }

    #[test]
    fn dwindle_allocates_three_windows_with_alternating_splits() {
        let ids = [1, 2, 3];
        let tree = build_dwindle_tree(&ids, SplitAxis::Horizontal, &[0.6, 0.5])
            .expect("tree should be built");
        let mut placements = Vec::new();
        allocate_dwindle(
            &tree,
            FloatRect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 40.0,
            },
            0.0,
            &mut placements,
        );

        let first = placement(&placements, 1);
        assert_close(first.x, 0.0);
        assert_close(first.y, 0.0);
        assert_close(first.w, 60.0);
        assert_close(first.h, 40.0);

        let second = placement(&placements, 2);
        assert_close(second.x, 60.0);
        assert_close(second.y, 0.0);
        assert_close(second.w, 40.0);
        assert_close(second.h, 20.0);

        let third = placement(&placements, 3);
        assert_close(third.x, 60.0);
        assert_close(third.y, 20.0);
        assert_close(third.w, 40.0);
        assert_close(third.h, 20.0);
    }

    fn two_tile_workspace(axis: SplitAxis) -> (Workspace, WindowId) {
        let mut workspace = Workspace::new(0);
        let mut next = 1;
        for app in [DemoApp::Terminal, DemoApp::Files] {
            seed_window(&mut workspace, &mut next, app, false, FloatRect::default());
        }
        // Force a known split so window 1 is the first (left/top) tile.
        workspace.tile_tree = Some(DwindleTree::Split {
            axis,
            ratio: 0.5,
            first: Box::new(DwindleTree::Leaf(1)),
            second: Box::new(DwindleTree::Leaf(2)),
        });
        (workspace, next)
    }

    #[test]
    fn tiled_resize_matches_cursor_horizontally() {
        let (workspace, next) = two_tile_workspace(SplitAxis::Horizontal);
        let viewport = Rect {
            x: 0,
            y: 0,
            w: 100,
            h: TOP_BAR_HEIGHT + 40,
        };
        let bounds = canvas_bounds_from_viewport(viewport);
        let before = placement(&workspace_target_rects(&workspace, bounds), 1).w;
        let mut state = state_with_workspace(workspace, next);

        // Drag window 1's lower-right corner (the divider) 8 cells to the right.
        resize_window(&mut state, 1, ResizeCorner::LowerRight, 8, 0, viewport);

        let after = placement(&workspace_target_rects(&state.workspaces[0], bounds), 1).w;
        assert_close(after - before, 8.0);
    }

    #[test]
    fn tiled_resize_matches_cursor_vertically() {
        let (workspace, next) = two_tile_workspace(SplitAxis::Vertical);
        let viewport = Rect {
            x: 0,
            y: 0,
            w: 100,
            h: TOP_BAR_HEIGHT + 40,
        };
        let bounds = canvas_bounds_from_viewport(viewport);
        let before = placement(&workspace_target_rects(&workspace, bounds), 1).h;
        let mut state = state_with_workspace(workspace, next);

        // Drag window 1's lower-right corner (the divider) 6 cells downward.
        resize_window(&mut state, 1, ResizeCorner::LowerRight, 0, 6, viewport);

        let after = placement(&workspace_target_rects(&state.workspaces[0], bounds), 1).h;
        assert_close(after - before, 6.0);
    }

    #[test]
    fn tiled_resize_ignores_corner_on_the_terminal_border() {
        let (workspace, next) = two_tile_workspace(SplitAxis::Horizontal);
        let viewport = Rect {
            x: 0,
            y: 0,
            w: 100,
            h: TOP_BAR_HEIGHT + 40,
        };
        let bounds = canvas_bounds_from_viewport(viewport);
        let before = placement(&workspace_target_rects(&workspace, bounds), 1);
        let mut state = state_with_workspace(workspace, next);

        // Window 1 is the left tile, flush against the terminal's left edge. Grabbing the
        // lower-left corner (on that border) must not resize — there is no divider there,
        // and the old code moved the inner divider in an inverted direction.
        resize_window(&mut state, 1, ResizeCorner::LowerLeft, 8, 0, viewport);

        let after = placement(&workspace_target_rects(&state.workspaces[0], bounds), 1);
        assert_close(after.w, before.w);
        assert_close(after.x, before.x);
    }

    #[test]
    fn floating_rect_may_hang_off_screen_but_stays_grabbable() {
        let bounds = FloatRect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 40.0,
        };
        let (w, h) = (30.0, 12.0);

        // Dragged far past the top-left: only the keep-visible margin remains on screen.
        let top_left = clamp_floating_rect(
            FloatRect {
                x: -500.0,
                y: -500.0,
                w,
                h,
            },
            bounds,
        );
        assert_close(top_left.x, OFFSCREEN_MIN_VISIBLE - w);
        assert_close(top_left.y, OFFSCREEN_MIN_VISIBLE - h);
        assert_close(top_left.w, w);
        assert_close(top_left.h, h);
        // The visible sliver equals the margin.
        assert_close(top_left.x + w, OFFSCREEN_MIN_VISIBLE);

        // Dragged far past the bottom-right: the left/top edge stops a margin in.
        let bottom_right = clamp_floating_rect(
            FloatRect {
                x: 500.0,
                y: 500.0,
                w,
                h,
            },
            bounds,
        );
        assert_close(bottom_right.x, bounds.w - OFFSCREEN_MIN_VISIBLE);
        assert_close(bottom_right.y, bounds.h - OFFSCREEN_MIN_VISIBLE);

        // A window comfortably inside is left exactly where it is.
        let inside = clamp_floating_rect(
            FloatRect {
                x: 20.0,
                y: 10.0,
                w,
                h,
            },
            bounds,
        );
        assert_close(inside.x, 20.0);
        assert_close(inside.y, 10.0);
    }

    #[test]
    fn lift_off_centers_remembered_size_on_the_tile() {
        let bounds = FloatRect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 40.0,
        };
        let tile = FloatRect {
            x: 50.0,
            y: 0.0,
            w: 49.0,
            h: 40.0,
        };
        // A "fixed default" floating rect parked in the top-left corner.
        let remembered = FloatRect {
            x: 5.0,
            y: 5.0,
            w: 30.0,
            h: 12.0,
        };

        let lifted = lift_off_float_rect(tile, remembered, bounds);

        // Remembered size is preserved...
        assert_close(lifted.w, remembered.w);
        assert_close(lifted.h, remembered.h);
        // ...but the window lifts off centered on the tile, not at the parked default.
        assert_close(lifted.x + lifted.w / 2.0, tile.x + tile.w / 2.0);
        assert_close(lifted.y + lifted.h / 2.0, tile.y + tile.h / 2.0);
    }

    fn spawn_bounds() -> FloatRect {
        canvas_bounds_from_viewport(Rect {
            x: 0,
            y: 0,
            w: 100,
            h: TOP_BAR_HEIGHT + 40,
        })
    }

    /// Push a tiled window into the windows list without adding it to the tree, matching
    /// the state `spawn_window` is in just before it calls `place_spawned_window`.
    fn push_bare_tiled_window(workspace: &mut Workspace, id: WindowId) {
        workspace.windows.push(WindowState {
            id,
            title: format!("w{id}"),
            app: DemoApp::Terminal,
            floating: false,
            fullscreen: false,
            floating_rect: FloatRect::default(),
            opening: true,
            closing: false,
        });
    }

    #[test]
    fn spawn_splits_the_focused_tile() {
        let (mut workspace, new_id) = two_tile_workspace(SplitAxis::Horizontal);
        let bounds = spawn_bounds();
        push_bare_tiled_window(&mut workspace, new_id);
        let one_before = placement(&workspace_target_rects(&workspace, bounds), 1);

        // Focus is on window 1 (the left tile, ~48x38 → taller than wide once the cell
        // aspect is applied), so the new window splits it top/bottom.
        let outcome = place_spawned_window(&mut workspace, new_id, Some(1), bounds);
        assert!(matches!(outcome, SpawnPlacement::Split(1)));

        let placements = workspace_target_rects(&workspace, bounds);
        let one_after = placement(&placements, 1);
        let new_rect = placement(&placements, new_id);
        // The new window took part of window 1's region (same column, stacked below).
        assert_close(new_rect.x, one_after.x);
        assert!(new_rect.y > one_after.y);
        assert!(one_after.h < one_before.h);
    }

    #[test]
    fn spawn_split_direction_follows_focused_tile_aspect() {
        // Wider than tall (after the cell-aspect multiplier) → split side by side.
        let wide = FloatRect {
            x: 0.0,
            y: 0.0,
            w: 80.0,
            h: 20.0,
        };
        assert_eq!(spawn_split_for_rect(wide).0, SplitAxis::Horizontal);

        // Taller/narrow → split top/bottom.
        let tall = FloatRect {
            x: 0.0,
            y: 0.0,
            w: 20.0,
            h: 30.0,
        };
        assert_eq!(spawn_split_for_rect(tall).0, SplitAxis::Vertical);

        // The new window always takes the second (right/bottom) slot — fixed side, not
        // cursor-dependent.
        assert!(!spawn_split_for_rect(wide).1);
    }

    #[test]
    fn split_ratio_math_clamps_before_splitting() {
        let rect = FloatRect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
        };

        let (small, rest) = split_float_rect(rect, SplitAxis::Horizontal, 0.01, 0.0);
        assert_close(small.w, 20.0);
        assert_close(rest.w, 80.0);

        let (large, rest) = split_float_rect(rect, SplitAxis::Horizontal, 0.99, 0.0);
        assert_close(large.w, 80.0);
        assert_close(rest.w, 20.0);
    }

    #[test]
    fn split_gap_is_reserved_between_neighbors() {
        let rect = FloatRect {
            x: 10.0,
            y: 2.0,
            w: 101.0,
            h: 20.0,
        };
        let (left, right) = split_float_rect(rect, SplitAxis::Horizontal, 0.5, 1.0);

        assert_close(left.x, 10.0);
        assert_close(left.w, 50.0);
        assert_close(right.x, 61.0);
        assert_close(right.w, 50.0);
    }

    #[test]
    fn nested_split_columns_render_flush_on_odd_extents() {
        // Mirrors the screenshot: a left leaf beside a right column that is itself split
        // vertically. With an odd usable height the right column used to round a cell
        // short, leaving a 2-row gap below it; tiles must instead line up with the leaf.
        let tree = DwindleTree::Split {
            axis: SplitAxis::Horizontal,
            ratio: 0.5,
            first: Box::new(DwindleTree::Leaf(1)),
            second: Box::new(DwindleTree::Split {
                axis: SplitAxis::Vertical,
                ratio: 0.5,
                first: Box::new(DwindleTree::Leaf(2)),
                second: Box::new(DwindleTree::Leaf(3)),
            }),
        };
        let rect = FloatRect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 24.0,
        };
        let mut placements = Vec::new();
        allocate_dwindle(&tree, rect, 1.0, &mut placements);

        let left = placement(&placements, 1).to_rect();
        let column_top = placement(&placements, 2).to_rect();
        let column_bottom = placement(&placements, 3).to_rect();

        let left_bottom = i32::from(left.y) + i32::from(left.h);
        let column_bottom_edge = i32::from(column_bottom.y) + i32::from(column_bottom.h);
        // The right column reaches the same bottom edge as the left leaf (no extra gap).
        assert_eq!(left_bottom, column_bottom_edge);
        assert_eq!(left_bottom, i32::from(rect.h as i16));
        // The two right-column tiles are separated by exactly the 1-cell gap.
        assert_eq!(
            i32::from(column_top.y) + i32::from(column_top.h) + 1,
            i32::from(column_bottom.y)
        );
    }

    #[test]
    fn split_adjustment_targets_nearest_focused_branch() {
        let ids = [11, 22, 33, 44];
        let mut tree = build_dwindle_tree(&ids, SplitAxis::Horizontal, &[0.5, 0.5, 0.5])
            .expect("tree should be built");

        assert_eq!(
            adjust_tree_split_for_focused(&mut tree, 44, 0.04, 0),
            Some(2)
        );

        let DwindleTree::Split { second, .. } = tree else {
            panic!("root should be split");
        };
        let DwindleTree::Split { second, .. } = *second else {
            panic!("second branch should be split");
        };
        let DwindleTree::Split { ratio, .. } = *second else {
            panic!("focused leaf should share a split");
        };

        assert_close(ratio, 0.46);
    }

    #[test]
    fn split_axis_toggle_targets_two_window_pair() {
        let ids = [1, 2];
        let mut tree =
            build_dwindle_tree(&ids, SplitAxis::Horizontal, &[0.5]).expect("tree should be built");

        assert_eq!(
            flip_tree_split_for_focused(&mut tree, 1, 0),
            Some((0, SplitAxis::Vertical))
        );
        let DwindleTree::Split { axis, .. } = tree else {
            panic!("tree should stay split");
        };

        assert_eq!(axis, SplitAxis::Vertical);
    }

    #[test]
    fn split_axis_toggle_treats_sibling_subtree_as_one_area() {
        let mut tree = DwindleTree::Split {
            axis: SplitAxis::Vertical,
            ratio: 0.5,
            first: Box::new(DwindleTree::Leaf(1)),
            second: Box::new(DwindleTree::Split {
                axis: SplitAxis::Horizontal,
                ratio: 0.5,
                first: Box::new(DwindleTree::Leaf(2)),
                second: Box::new(DwindleTree::Leaf(3)),
            }),
        };

        assert_eq!(
            flip_tree_split_for_focused(&mut tree, 1, 0),
            Some((0, SplitAxis::Horizontal))
        );

        let mut placements = Vec::new();
        allocate_dwindle(
            &tree,
            FloatRect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 40.0,
            },
            0.0,
            &mut placements,
        );
        let focused = placement(&placements, 1);
        let lower_left = placement(&placements, 2);
        let lower_right = placement(&placements, 3);

        assert_close(focused.x, 0.0);
        assert_close(focused.w, 50.0);
        assert_close(lower_left.x, 50.0);
        assert_close(lower_right.x, 75.0);
        assert_close(lower_left.y, lower_right.y);
    }

    #[test]
    fn split_axis_toggle_inside_group_leaves_parent_axis_unchanged() {
        let mut tree = DwindleTree::Split {
            axis: SplitAxis::Vertical,
            ratio: 0.5,
            first: Box::new(DwindleTree::Leaf(1)),
            second: Box::new(DwindleTree::Split {
                axis: SplitAxis::Horizontal,
                ratio: 0.5,
                first: Box::new(DwindleTree::Leaf(2)),
                second: Box::new(DwindleTree::Leaf(3)),
            }),
        };

        assert_eq!(
            flip_tree_split_for_focused(&mut tree, 2, 0),
            Some((1, SplitAxis::Vertical))
        );

        let DwindleTree::Split {
            axis: root_axis,
            second,
            ..
        } = tree
        else {
            panic!("root should be split");
        };
        let DwindleTree::Split {
            axis: child_axis, ..
        } = *second
        else {
            panic!("child should be split");
        };

        assert_eq!(root_axis, SplitAxis::Vertical);
        assert_eq!(child_axis, SplitAxis::Vertical);
    }

    #[test]
    fn alt_space_toggles_focused_split_through_key_path() {
        let mut backend = TestBackend::new(WindowManagerDemo::default());
        backend
            .dispatch(Msg::FocusWindow(1, FrameworkFocus::Request))
            .expect("focus message should dispatch");

        assert!(
            backend
                .send_key(KeyEvent {
                    code: KeyCode::Char(' '),
                    mods: KeyMods {
                        alt: true,
                        ..KeyMods::NONE
                    },
                })
                .expect("Alt+Space should dispatch"),
            "Alt+Space should be handled by the WM"
        );

        assert_eq!(
            workspace_root_axis(&backend.state().workspaces[0]),
            SplitAxis::Vertical
        );
        assert!(backend.state().status.contains("Flipped focused split"));
    }

    #[test]
    fn ratio_adjustment_stays_inside_safe_bounds() {
        assert_close(adjust_ratio_value(0.22, -0.10), MIN_SPLIT_RATIO);
        assert_close(adjust_ratio_value(0.78, 0.10), MAX_SPLIT_RATIO);
        assert_close(adjust_ratio_value(0.50, 0.04), 0.54);
    }

    #[test]
    fn shifted_workspace_symbols_request_move_semantics() {
        let key = KeyEvent {
            code: KeyCode::Char('!'),
            mods: KeyMods {
                alt: true,
                ..KeyMods::NONE
            },
        };

        assert_eq!(workspace_key(key), Some((0, true)));
    }

    #[test]
    fn hover_focus_is_default_policy() {
        assert_eq!(
            WindowManagerConfig::default().focus_mode,
            WindowFocusMode::Hover
        );
    }

    #[test]
    fn wm_focus_request_moves_framework_focus_inside_window() {
        let mut backend = TestBackend::new(WindowManagerDemo::default());

        backend
            .dispatch(Msg::FocusWindow(2, FrameworkFocus::Request))
            .expect("focus message should dispatch");

        assert_eq!(backend.state().focused_window, Some(2));
        assert_eq!(
            backend.focused_key().map(|key| key.as_ref()),
            Some("wm-window-widget-2-files")
        );
    }

    #[test]
    fn tiled_drag_preview_uses_remembered_float_size() {
        let mut backend = TestBackend::new(WindowManagerDemo::default());
        let remembered = FloatRect {
            x: 8.0,
            y: 4.0,
            w: 32.0,
            h: 9.0,
        };
        backend.state_mut().workspaces[0].windows[0].floating_rect = remembered;
        let bounds = canvas_bounds_from_viewport(backend.viewport());
        let tile_rect = placement(
            &workspace_target_rects(&backend.state().workspaces[0], bounds),
            1,
        );

        backend
            .dispatch(Msg::BeginMove(
                1,
                tile_rect,
                (tile_rect.w / 2.0).round() as u16,
                (tile_rect.h / 2.0).round() as u16,
                tile_rect.w.round() as u16,
                tile_rect.h.round() as u16,
                true,
            ))
            .expect("begin move should dispatch");

        let session = backend.state().moving_window.expect("move session");
        assert!(!session.was_floating);
        assert_close(session.drag_rect.w, remembered.w);
        assert_close(session.drag_rect.h, remembered.h);
        assert_close(
            backend.state().workspaces[0].windows[0].floating_rect.w,
            remembered.w,
        );
        assert_close(
            backend.state().workspaces[0].windows[0].floating_rect.h,
            remembered.h,
        );
        assert_eq!(backend.state().animation, GeometryAnimation::TileFloat);
    }

    #[test]
    fn tile_to_float_preserves_remembered_float_dimensions() {
        let mut backend = TestBackend::new(WindowManagerDemo::default());
        let remembered = FloatRect {
            x: 10.0,
            y: 5.0,
            w: 28.0,
            h: 11.0,
        };
        backend.state_mut().workspaces[0].windows[0].floating_rect = remembered;
        backend
            .dispatch(Msg::FocusWindow(1, FrameworkFocus::Request))
            .expect("focus message should dispatch");

        assert!(
            backend
                .send_key(KeyEvent {
                    code: KeyCode::Char('t'),
                    mods: KeyMods {
                        alt: true,
                        ..KeyMods::NONE
                    },
                })
                .expect("Alt+T should dispatch"),
            "Alt+T should be handled by the WM"
        );

        let window = &backend.state().workspaces[0].windows[0];
        assert!(window.floating);
        assert_close(window.floating_rect.w, remembered.w);
        assert_close(window.floating_rect.h, remembered.h);
    }

    #[test]
    fn fullscreen_tiled_window_preserves_remembered_float_dimensions() {
        let mut backend = TestBackend::new(WindowManagerDemo::default());
        let remembered = FloatRect {
            x: 12.0,
            y: 6.0,
            w: 34.0,
            h: 12.0,
        };
        backend.state_mut().workspaces[0].windows[0].floating_rect = remembered;
        backend
            .dispatch(Msg::FocusWindow(1, FrameworkFocus::Request))
            .expect("focus message should dispatch");

        assert!(
            backend
                .send_key(KeyEvent {
                    code: KeyCode::Char('f'),
                    mods: KeyMods {
                        alt: true,
                        ..KeyMods::NONE
                    },
                })
                .expect("Alt+F should dispatch"),
            "Alt+F should be handled by the WM"
        );
        assert!(backend.state().workspaces[0].windows[0].fullscreen);
        assert_close(
            backend.state().workspaces[0].windows[0].floating_rect.w,
            remembered.w,
        );
        assert_close(
            backend.state().workspaces[0].windows[0].floating_rect.h,
            remembered.h,
        );
    }

    #[test]
    fn titlebar_background_hides_frame_corner() {
        let mut backend = TestBackend::new(WindowManagerDemo {
            config: WindowManagerConfig {
                animations: WindowAnimationConfig {
                    focus_chrome: false,
                    ..WindowAnimationConfig::default()
                },
                ..WindowManagerConfig::default()
            },
        });
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 120,
            h: 40,
        });
        backend.render();
        backend
            .dispatch(Msg::FocusWindow(1, FrameworkFocus::Request))
            .expect("focus message should dispatch");

        let bounds = canvas_bounds_from_viewport(backend.viewport());
        let placements = workspace_target_rects(&backend.state().workspaces[0], bounds);
        let rect = placement(&placements, 1).to_rect();
        let x = rect.x.max(0) as u16 + rect.w.saturating_sub(1);
        let y = TOP_BAR_HEIGHT + rect.y.max(0) as u16;
        let frame = backend.capture_frame();

        assert_eq!(frame.cell(x, y).symbol, " ");
        assert_eq!(frame.cell(x, y).bg, Color::rgb(124, 207, 255));
    }

    #[test]
    fn floating_titlebar_background_hides_merged_corner_glyphs() {
        let mut backend = TestBackend::new(WindowManagerDemo {
            config: WindowManagerConfig {
                animations: WindowAnimationConfig {
                    focus_chrome: false,
                    ..WindowAnimationConfig::default()
                },
                ..WindowManagerConfig::default()
            },
        });
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 120,
            h: 40,
        });
        backend.render();
        backend
            .dispatch(Msg::FocusWindow(4, FrameworkFocus::Request))
            .expect("focus message should dispatch");

        let bounds = canvas_bounds_from_viewport(backend.viewport());
        let placements = workspace_target_rects(&backend.state().workspaces[0], bounds);
        let rect = placement(&placements, 4).to_rect();
        let y = TOP_BAR_HEIGHT + rect.y.max(0) as u16;
        let left_x = rect.x.max(0) as u16;
        let right_x = rect.x.max(0) as u16 + rect.w.saturating_sub(1);
        let frame = backend.capture_frame();

        for x in [left_x, right_x] {
            assert_eq!(frame.cell(x, y).symbol, " ");
            assert_eq!(frame.cell(x, y).bg, Color::rgb(124, 207, 255));
        }
    }

    #[test]
    fn canvas_local_drop_point_accounts_for_top_bar() {
        let bounds = FloatRect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 40.0,
        };

        assert_eq!(
            canvas_local_point_from_mouse(12, TOP_BAR_HEIGHT + 7, bounds),
            (12.0, 7.0)
        );
    }

    #[test]
    fn tiled_drop_target_prefers_rect_containing_pointer() {
        let tiled_ids = [1, 2, 3];
        let placements = [
            WindowPlacement {
                id: 1,
                rect: FloatRect {
                    x: 0.0,
                    y: 0.0,
                    w: 30.0,
                    h: 20.0,
                },
            },
            WindowPlacement {
                id: 2,
                rect: FloatRect {
                    x: 31.0,
                    y: 0.0,
                    w: 30.0,
                    h: 20.0,
                },
            },
            WindowPlacement {
                id: 3,
                rect: FloatRect {
                    x: 62.0,
                    y: 0.0,
                    w: 30.0,
                    h: 20.0,
                },
            },
        ];

        assert_eq!(
            target_tiled_window_for_drop(&placements, &tiled_ids, (70.0, 5.0)),
            Some(3)
        );
    }

    #[test]
    fn tiled_drop_target_can_be_original_slot() {
        let tiled_ids = [1, 2, 3];
        let placements = [
            WindowPlacement {
                id: 1,
                rect: FloatRect {
                    x: 0.0,
                    y: 0.0,
                    w: 30.0,
                    h: 20.0,
                },
            },
            WindowPlacement {
                id: 2,
                rect: FloatRect {
                    x: 31.0,
                    y: 0.0,
                    w: 30.0,
                    h: 20.0,
                },
            },
            WindowPlacement {
                id: 3,
                rect: FloatRect {
                    x: 62.0,
                    y: 0.0,
                    w: 30.0,
                    h: 20.0,
                },
            },
        ];

        assert_eq!(
            target_tiled_window_for_drop(&placements, &tiled_ids, (40.0, 5.0)),
            Some(2)
        );
    }

    #[test]
    fn tiled_drop_target_ignores_gaps_and_floating_overlays() {
        let tiled_ids = [1, 2];
        let placements = [
            WindowPlacement {
                id: 1,
                rect: FloatRect {
                    x: 0.0,
                    y: 0.0,
                    w: 10.0,
                    h: 10.0,
                },
            },
            WindowPlacement {
                id: 2,
                rect: FloatRect {
                    x: 20.0,
                    y: 0.0,
                    w: 10.0,
                    h: 10.0,
                },
            },
            WindowPlacement {
                id: 9,
                rect: FloatRect {
                    x: 0.0,
                    y: 0.0,
                    w: 10.0,
                    h: 10.0,
                },
            },
        ];

        assert_eq!(
            target_tiled_window_for_drop(&placements, &tiled_ids, (15.0, 5.0)),
            None
        );
        assert_eq!(
            target_tiled_window_for_drop(&placements, &tiled_ids, (5.0, 5.0)),
            None
        );
        assert_eq!(
            target_tiled_window_for_drop(&placements, &tiled_ids, (25.0, 5.0)),
            Some(2)
        );
    }

    #[test]
    fn drop_split_uses_pointer_half_on_full_height_target() {
        let target = FloatRect {
            x: 1.0,
            y: 1.0,
            w: 58.0,
            h: 38.0,
        };

        assert_eq!(
            drop_split_for_target(target, (30.0, 3.0)),
            (SplitAxis::Vertical, true)
        );
        assert_eq!(
            drop_split_for_target(target, (30.0, 37.0)),
            (SplitAxis::Vertical, false)
        );
        assert_eq!(
            drop_split_for_target(target, (3.0, 20.0)),
            (SplitAxis::Horizontal, true)
        );
        assert_eq!(
            drop_split_for_target(target, (57.0, 20.0)),
            (SplitAxis::Horizontal, false)
        );
    }

    #[test]
    fn tiled_drop_path_splits_full_height_left_target_top_bottom() {
        let mut workspace = Workspace::new(0);
        let mut next = 1;
        seed_window(
            &mut workspace,
            &mut next,
            DemoApp::Terminal,
            false,
            FloatRect::default(),
        );
        seed_window(
            &mut workspace,
            &mut next,
            DemoApp::Files,
            false,
            FloatRect::default(),
        );
        seed_window(
            &mut workspace,
            &mut next,
            DemoApp::Metrics,
            false,
            FloatRect::default(),
        );

        let viewport = Rect {
            x: 0,
            y: 0,
            w: 100,
            h: TOP_BAR_HEIGHT + 40,
        };
        let bounds = canvas_bounds_from_viewport(viewport);
        let target = placement(
            &workspace_target_rects_excluding(&workspace, bounds, Some(3)),
            1,
        );
        let drop_x = (target.x + target.w / 2.0).round() as u16;
        let drop_y = (target.y + 1.0).round() as u16 + TOP_BAR_HEIGHT;
        let mut state = state_with_workspace(workspace, next);

        drop_tiled_window_at(&mut state, 3, drop_x, drop_y, viewport);

        assert_eq!(state.animation, GeometryAnimation::TileFloat);
        let placements = workspace_target_rects(&state.workspaces[0], bounds);
        let moved = placement(&placements, 3);
        let original_target = placement(&placements, 1);
        let right = placement(&placements, 2);

        assert_close(moved.x, original_target.x);
        assert_close(moved.w, original_target.w);
        assert!(
            moved.y < original_target.y,
            "moved tile should be above target"
        );
        assert!(right.x > original_target.x + original_target.w);
        assert!(right.h > 30.0, "right tile should keep the full height");
    }

    #[test]
    fn float_to_tile_inserts_at_floating_window_center() {
        let mut workspace = Workspace::new(0);
        let mut next = 1;
        seed_window(
            &mut workspace,
            &mut next,
            DemoApp::Terminal,
            false,
            FloatRect::default(),
        );
        seed_window(
            &mut workspace,
            &mut next,
            DemoApp::Files,
            false,
            FloatRect::default(),
        );
        seed_window(
            &mut workspace,
            &mut next,
            DemoApp::Chat,
            true,
            FloatRect {
                x: 20.0,
                y: 2.0,
                w: 12.0,
                h: 8.0,
            },
        );

        let bounds = FloatRect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 40.0,
        };
        let floating_rect = workspace.windows[2].floating_rect;
        let point = rect_center(clamp_float_rect(floating_rect, bounds));
        workspace.windows[2].floating = false;

        assert_eq!(
            insert_tiled_window_at_point(&mut workspace, 3, point, bounds),
            Some((1, true))
        );

        let placements = workspace_target_rects(&workspace, bounds);
        let moved = placement(&placements, 3);
        let target = placement(&placements, 1);
        let right = placement(&placements, 2);

        assert_close(moved.x, target.x);
        assert_close(moved.w, target.w);
        assert!(
            moved.y < target.y,
            "floating window should tile above target"
        );
        assert!(
            right.x > target.x + target.w,
            "right tile should stay on the right"
        );
    }

    #[test]
    fn excluding_dragged_tiled_window_reflows_remaining_tiles() {
        let mut workspace = Workspace::new(0);
        let mut next = 1;
        seed_window(
            &mut workspace,
            &mut next,
            DemoApp::Terminal,
            false,
            FloatRect::default(),
        );
        seed_window(
            &mut workspace,
            &mut next,
            DemoApp::Files,
            false,
            FloatRect::default(),
        );
        seed_window(
            &mut workspace,
            &mut next,
            DemoApp::Metrics,
            false,
            FloatRect::default(),
        );

        let bounds = FloatRect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 40.0,
        };
        let placements = workspace_target_rects_excluding(&workspace, bounds, Some(3));
        let first = placement(&placements, 1);
        let second = placement(&placements, 2);

        assert!(placement_for(&placements, 3).is_none());
        assert_close(first.h, second.h);
        assert!(second.h > 30.0, "remaining right tile should fill height");
    }

    #[test]
    fn tiled_drop_splits_target_leaf_instead_of_swapping() {
        let mut workspace = Workspace::new(0);
        let mut next = 1;
        seed_window(
            &mut workspace,
            &mut next,
            DemoApp::Terminal,
            false,
            FloatRect::default(),
        );
        seed_window(
            &mut workspace,
            &mut next,
            DemoApp::Files,
            false,
            FloatRect::default(),
        );
        seed_window(
            &mut workspace,
            &mut next,
            DemoApp::Metrics,
            false,
            FloatRect::default(),
        );

        assert!(move_tiled_window_around_target(
            &mut workspace,
            3,
            1,
            SplitAxis::Vertical,
            false,
        ));

        let placements = workspace_target_rects(
            &workspace,
            FloatRect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 40.0,
            },
        );
        let first = placement(&placements, 1);
        let moved = placement(&placements, 3);
        let right = placement(&placements, 2);

        assert_close(first.x, moved.x);
        assert_close(first.w, moved.w);
        assert!(
            moved.y > first.y,
            "moved tile should share the left side below target"
        );
        assert!(
            right.x > first.x + first.w,
            "remaining tile should stay on the right side"
        );
        assert!(
            right.h > 30.0,
            "remaining right tile should take full height"
        );
    }

    #[test]
    fn tiled_tree_preserves_floating_slots() {
        let mut workspace = Workspace::new(0);
        let mut next = 1;
        seed_window(
            &mut workspace,
            &mut next,
            DemoApp::Terminal,
            false,
            FloatRect::default(),
        );
        seed_window(
            &mut workspace,
            &mut next,
            DemoApp::Chat,
            true,
            FloatRect::default(),
        );
        seed_window(
            &mut workspace,
            &mut next,
            DemoApp::Files,
            false,
            FloatRect::default(),
        );
        seed_window(
            &mut workspace,
            &mut next,
            DemoApp::Metrics,
            false,
            FloatRect::default(),
        );

        assert_eq!(workspace.tiled_ids(), vec![1, 3, 4]);
        assert!(move_tiled_window_around_target(
            &mut workspace,
            1,
            4,
            SplitAxis::Vertical,
            false,
        ));

        assert_eq!(workspace.tiled_ids(), vec![3, 4, 1]);
        assert!(
            workspace.windows[1].floating,
            "floating window slot should stay put"
        );
        assert_eq!(workspace.windows[1].id, 2);
    }

    #[test]
    fn resize_corner_uses_drag_start_position() {
        assert_eq!(
            nearest_resize_corner_from_local(1, 1, 80, 24),
            ResizeCorner::UpperLeft
        );
        assert_eq!(
            nearest_resize_corner_from_local(78, 1, 80, 24),
            ResizeCorner::UpperRight
        );
        assert_eq!(
            nearest_resize_corner_from_local(1, 22, 80, 24),
            ResizeCorner::LowerLeft
        );
        assert_eq!(
            nearest_resize_corner_from_local(78, 22, 80, 24),
            ResizeCorner::LowerRight
        );
    }

    #[test]
    fn upper_left_resize_preserves_opposite_corner() {
        let bounds = FloatRect {
            x: 0.0,
            y: 0.0,
            w: 120.0,
            h: 60.0,
        };
        let rect = FloatRect {
            x: 20.0,
            y: 10.0,
            w: 50.0,
            h: 20.0,
        };

        let resized =
            resize_float_rect_from_corner(rect, ResizeCorner::UpperLeft, -5.0, -3.0, bounds);

        assert_close(resized.x, 15.0);
        assert_close(resized.y, 7.0);
        assert_close(resized.w, 55.0);
        assert_close(resized.h, 23.0);
        assert_close(resized.x + resized.w, rect.x + rect.w);
        assert_close(resized.y + resized.h, rect.y + rect.h);
    }

    #[test]
    fn directional_focus_prefers_side_neighbor_over_diagonal() {
        let current = FloatRect {
            x: 60.0,
            y: 21.0,
            w: 40.0,
            h: 19.0,
        };
        let left = FloatRect {
            x: 0.0,
            y: 0.0,
            w: 59.0,
            h: 40.0,
        };
        let above = FloatRect {
            x: 60.0,
            y: 0.0,
            w: 40.0,
            h: 20.0,
        };

        let left_score = directional_score(current, left, Direction::Left).expect("left candidate");
        assert!(
            directional_score(current, above, Direction::Left).is_none(),
            "an above pane should not be a left-arrow candidate"
        );
        assert!(
            left_score < 100.0,
            "overlapping side neighbor should score well"
        );
    }

    #[test]
    fn disabled_animation_uses_zero_delays() {
        let animations = WindowAnimationConfig {
            enabled: false,
            ..WindowAnimationConfig::default()
        };

        assert_eq!(open_delay(animations), Duration::ZERO);
        assert_eq!(close_delay(animations), Duration::ZERO);
    }

    #[test]
    fn close_delay_waits_for_survivor_geometry_animation() {
        let animations = WindowAnimationConfig {
            close_duration: Duration::from_millis(80),
            geometry_duration: Duration::from_millis(240),
            ..WindowAnimationConfig::default()
        };

        assert_eq!(close_delay(animations), Duration::from_millis(260));
    }
}
