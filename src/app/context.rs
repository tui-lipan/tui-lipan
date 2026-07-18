use crate::app::input::key_dispatch::{
    ChordMismatchPolicy, CommandConflictPolicy, KeyDispatchPolicy, TerminalKeyPolicy,
};
use crate::app::input::keymap::{FrameworkAction, FrameworkKeymap, UserKeymapPolicy};
#[cfg(not(target_arch = "wasm32"))]
use crate::app::runner::AppRunner;
use crate::clipboard::{ClipboardConfig, ClipboardError, ClipboardProvider, ClipboardReporter};
#[cfg(not(target_arch = "wasm32"))]
use crate::core::component::Component;
use crate::input::KeyBindings;
use crate::layout::tag::Tag;
use crate::overlay::ToastPlacement;
use crate::style::Padding;
use crate::style::{Color, Paint, Style, Theme};
use std::path::PathBuf;

/// How the app occupies terminal space.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum ViewportMode {
    /// Take over the full terminal using the alternate screen.
    #[default]
    Fullscreen,
    /// Render inline at the current cursor position.
    Inline { height: InlineHeight },
}

/// Height policy for inline viewports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlineHeight {
    /// A fixed number of rows, clamped to at least one row.
    Fixed(u16),
    /// Follow the content's measured height, re-sizing the viewport as the
    /// view changes.
    Auto {
        /// Optional row cap. Regardless of the cap, the viewport never grows
        /// past the host terminal height.
        max: Option<u16>,
    },
}

impl InlineHeight {
    /// Content-sized height, capped only by the host terminal height.
    pub const fn auto() -> Self {
        Self::Auto { max: None }
    }

    /// Content-sized height, capped at `max` rows.
    pub const fn auto_capped(max: u16) -> Self {
        Self::Auto { max: Some(max) }
    }

    pub(crate) fn normalized(self) -> Self {
        match self {
            Self::Fixed(rows) => Self::Fixed(rows.max(1)),
            Self::Auto { max } => Self::Auto {
                max: max.map(|rows| rows.max(1)),
            },
        }
    }

    /// Rows to reserve for the viewport before the first frame is measured.
    pub(crate) fn initial_rows(self) -> u16 {
        match self {
            Self::Fixed(rows) => rows.max(1),
            // Auto starts minimal: the first render measures the content and
            // grows the viewport before anything is painted.
            Self::Auto { .. } => 1,
        }
    }
}

impl From<u16> for InlineHeight {
    fn from(rows: u16) -> Self {
        Self::Fixed(rows)
    }
}

/// Startup behavior for transcript inline mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InlineStartupPolicy {
    /// Preserve host terminal content above the inline viewport.
    #[default]
    PreserveHost,
    /// Clear the host terminal before the first inline render.
    ClearHost,
}

/// Public surface mode taxonomy for app rendering.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SurfaceMode {
    /// Take over the full terminal using the alternate screen.
    #[default]
    Fullscreen,
    /// Inline viewport intended for ephemeral UI sessions.
    InlineEphemeral {
        /// Requested inline viewport height.
        height: InlineHeight,
    },
    /// Inline viewport intended for transcript-friendly sessions.
    InlineTranscript {
        /// Requested inline viewport height.
        height: InlineHeight,
        /// Startup behavior for the host terminal.
        startup: InlineStartupPolicy,
    },
}

impl SurfaceMode {
    pub(crate) fn is_inline(&self) -> bool {
        !matches!(self, Self::Fullscreen)
    }

    pub(crate) fn normalized(self) -> Self {
        match self {
            Self::Fullscreen => Self::Fullscreen,
            Self::InlineEphemeral { height } => Self::InlineEphemeral {
                height: height.normalized(),
            },
            Self::InlineTranscript { height, startup } => Self::InlineTranscript {
                height: height.normalized(),
                startup,
            },
        }
    }

    pub(crate) fn viewport_mode(self) -> ViewportMode {
        match self.normalized() {
            Self::Fullscreen => ViewportMode::Fullscreen,
            Self::InlineEphemeral { height } | Self::InlineTranscript { height, .. } => {
                ViewportMode::Inline { height }
            }
        }
    }

    pub(crate) fn clear_on_start(self) -> bool {
        matches!(
            self,
            Self::InlineTranscript {
                startup: InlineStartupPolicy::ClearHost,
                ..
            }
        )
    }
}

/// Controls which Enter key combinations insert new lines in `TextArea`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAreaNewlineBinding {
    /// Use plain Enter.
    #[default]
    Enter,
    /// Use Shift+Enter only.
    ShiftEnter,
    /// Accept both Enter and Shift+Enter.
    EnterOrShiftEnter,
}

/// Controls framework-initiated focus movement.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FocusPolicy {
    /// Focus the first focusable widget at startup and whenever focus cannot be restored.
    Auto,
    /// Start unfocused, then allow Tab and pointer interaction to establish focus.
    #[default]
    OnDemand,
    /// Never move focus through global traversal or pointer interaction.
    ///
    /// Explicit focus requests and focus traversal helpers remain available. Capturing overlays
    /// also continue to establish and trap focus.
    Manual,
}

/// Public identity of a focused widget at a focus transition boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FocusEntry {
    /// Optional stable widget key.
    pub key: Option<crate::core::element::Key>,
    /// Widget kind.
    pub tag: Tag,
}

/// App-level focus transition payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FocusChanged {
    /// Previously focused widget, if any.
    pub old: Option<FocusEntry>,
    /// Newly focused widget, if any.
    pub new: Option<FocusEntry>,
}

pub(crate) type FocusChangedHook = std::rc::Rc<dyn Fn(&FocusChanged)>;

/// Controls automatic foreground contrast adjustments for widget text.
#[cfg_attr(
    feature = "terminal-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ContrastPolicy {
    /// Keep user-provided foreground colors unchanged.
    Off,
    /// Auto-adjust foreground on colored backgrounds when contrast is too low
    /// using WCAG 2.1 contrast ratio (AA normal text: >= 4.5:1).
    #[default]
    Wcag,
    /// Keep the current foreground when it is readable under WCAG 2.1;
    /// otherwise snap to black or white, whichever has higher contrast.
    BlackOrWhite,
    /// Auto-adjust using APCA perceptual contrast (WCAG 3.0 draft).
    ///
    /// Better for dark themes and polarity-aware readability. Uses `|Lc|` >= 60
    /// as the minimum body-text threshold.
    Apca,
}

#[cfg(all(test, feature = "terminal-serde"))]
mod terminal_serde_tests {
    use super::*;

    #[test]
    fn contrast_policy_round_trips() {
        let policy = ContrastPolicy::BlackOrWhite;
        let json = serde_json::to_string(&policy).unwrap();
        assert_eq!(
            serde_json::from_str::<ContrastPolicy>(&json).unwrap(),
            policy
        );
    }
}

/// Runtime devtools subsystem configuration.
#[cfg(feature = "devtools")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DevToolsConfig {
    /// Enable devtools log ingestion and sink wiring.
    pub logs: bool,
    /// Enable runtime frame metrics collection.
    pub metrics: bool,
    /// Show tui-lipan's own framework-internal logs in the log view.
    ///
    /// Set to `false` so the devtools log view starts with framework noise
    /// (key events, dirty tracking, etc.) hidden, showing only the host
    /// application's own `debug_log!` lines. Can still be toggled at runtime
    /// from the "tui-lipan" button in the Logs tab.
    pub show_framework_logs: bool,
}

#[cfg(feature = "devtools")]
impl Default for DevToolsConfig {
    fn default() -> Self {
        Self {
            logs: true,
            metrics: true,
            show_framework_logs: true,
        }
    }
}

/// How the root viewport background is painted before the UI tree renders.
///
/// By default the framework paints nothing behind the tree, so the host
/// terminal background shows through ([`Transparent`](Self::Transparent)). Opt
/// into a filled background when you want a fully "designed" surface rather than
/// text floating on the terminal color — useful for kiosk-style apps, themes
/// with a strong identity, or matching a brand backdrop.
///
/// ```no_run
/// use tui_lipan::prelude::*;
///
/// // Fill with the active theme's backdrop surface:
/// let app = App::new().theme(Theme::lipan()).fill_background();
///
/// // Or an explicit color:
/// let app = App::new().screen_background(Color::hex_u24(0x04090D));
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ScreenBackground {
    /// Leave the host terminal background untouched. (default)
    #[default]
    Transparent,
    /// Fill the viewport with the active root theme's backdrop surface
    /// ([`Theme::surface`]`.backdrop`).
    ///
    /// This tracks the theme of the realized root node, so apps that swap themes
    /// at runtime via a root `ThemeProvider` (rather than rebuilding the `App`)
    /// keep the backdrop in sync without any extra wiring.
    Theme,
    /// Fill the viewport with an explicit style (typically just a background color).
    Custom(Style),
}

impl ScreenBackground {
    /// Resolve to the concrete fill style for `theme`, or `None` when nothing
    /// should be painted.
    pub(crate) fn resolve(self, theme: &Theme) -> Option<Style> {
        match self {
            Self::Transparent => None,
            Self::Theme => Some(Style::new().bg(theme.surface.backdrop)),
            Self::Custom(style) => (!style.is_empty()).then_some(style),
        }
    }
}

impl From<Style> for ScreenBackground {
    fn from(style: Style) -> Self {
        Self::Custom(style)
    }
}

impl From<Color> for ScreenBackground {
    fn from(color: Color) -> Self {
        Self::Custom(Style::new().bg(color))
    }
}

impl From<Paint> for ScreenBackground {
    fn from(paint: Paint) -> Self {
        Self::Custom(Style::new().bg(paint))
    }
}

/// Application builder.
pub struct App {
    pub(crate) title: Option<String>,
    pub(crate) surface_mode: SurfaceMode,
    pub(crate) mouse_enabled: Option<bool>,
    pub(crate) scroll_wheel_multiplier: u16,
    pub(crate) theme: Theme,
    pub(crate) toast_placement: ToastPlacement,
    pub(crate) toast_gap: u16,
    pub(crate) toast_margin: Padding,
    pub(crate) clipboard_config: ClipboardConfig,
    pub(crate) keymap_path: Option<PathBuf>,
    pub(crate) framework_keymap: FrameworkKeymap,
    pub(crate) user_keymap_policy: UserKeymapPolicy,
    pub(crate) key_dispatch_policy: KeyDispatchPolicy,
    pub(crate) focus_policy: FocusPolicy,
    pub(crate) on_focus_changed: Option<FocusChangedHook>,
    pub(crate) terminal_key_policy: TerminalKeyPolicy,
    pub(crate) command_conflict_policy: CommandConflictPolicy,
    pub(crate) chord_mismatch_policy: ChordMismatchPolicy,
    pub(crate) text_area_newline_binding: TextAreaNewlineBinding,
    pub(crate) contrast_policy: ContrastPolicy,
    pub(crate) clipboard_provider: Option<Box<dyn ClipboardProvider>>,
    pub(crate) clipboard_reporter: ClipboardReporter,
    pub(crate) terminal_bg: Option<Color>,
    pub(crate) live_host_terminal_colors: bool,
    pub(crate) system_theme: bool,
    pub(crate) screen_background: ScreenBackground,
    #[cfg(feature = "devtools")]
    pub(crate) devtools_config: DevToolsConfig,
}

impl Default for App {
    fn default() -> Self {
        let theme = Theme::default();
        Self {
            title: None,
            surface_mode: SurfaceMode::default(),
            mouse_enabled: None,
            scroll_wheel_multiplier: 1,
            theme,
            toast_placement: ToastPlacement::default(),
            toast_gap: 1,
            toast_margin: Padding::BORDER,
            clipboard_config: ClipboardConfig::default(),
            keymap_path: None,
            framework_keymap: FrameworkKeymap::default(),
            user_keymap_policy: UserKeymapPolicy::default(),
            key_dispatch_policy: KeyDispatchPolicy::WidgetFirst,
            focus_policy: FocusPolicy::default(),
            on_focus_changed: None,
            terminal_key_policy: TerminalKeyPolicy::FrameworkFirst,
            command_conflict_policy: CommandConflictPolicy::default(),
            chord_mismatch_policy: ChordMismatchPolicy::default(),
            text_area_newline_binding: TextAreaNewlineBinding::default(),
            contrast_policy: ContrastPolicy::default(),
            clipboard_provider: None,
            clipboard_reporter: crate::clipboard::default_clipboard_reporter(),
            terminal_bg: None,
            live_host_terminal_colors: false,
            system_theme: false,
            screen_background: ScreenBackground::default(),
            #[cfg(feature = "devtools")]
            devtools_config: DevToolsConfig::default(),
        }
    }
}

impl App {
    /// Create a new app.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the terminal window title (via OSC 2 escape sequence).
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set the app surface mode explicitly.
    pub fn surface(mut self, mode: SurfaceMode) -> Self {
        self.surface_mode = mode.normalized();
        self
    }

    /// Use fullscreen alternate-screen rendering.
    pub fn fullscreen(self) -> Self {
        self.surface(SurfaceMode::Fullscreen)
    }

    /// Render the app inline for ephemeral (non-transcript) sessions.
    ///
    /// Accepts a fixed row count (clamped to at least one row) or an
    /// [`InlineHeight`] policy such as [`InlineHeight::auto()`], which sizes
    /// the viewport to the content every frame.
    pub fn inline_ephemeral(self, height: impl Into<InlineHeight>) -> Self {
        self.surface(SurfaceMode::InlineEphemeral {
            height: height.into(),
        })
    }

    /// Render the app inline for transcript sessions.
    ///
    /// Accepts a fixed row count or an [`InlineHeight`] policy such as
    /// [`InlineHeight::auto()`]. Defaults to preserving host terminal content
    /// on startup.
    pub fn inline_transcript(self, height: impl Into<InlineHeight>) -> Self {
        self.surface(SurfaceMode::InlineTranscript {
            height: height.into(),
            startup: InlineStartupPolicy::PreserveHost,
        })
    }

    /// Render the app inline for transcript sessions with explicit startup behavior.
    ///
    /// Accepts a fixed row count (clamped to at least one row) or an
    /// [`InlineHeight`] policy such as [`InlineHeight::auto()`].
    pub fn inline_transcript_with_startup(
        self,
        height: impl Into<InlineHeight>,
        startup: InlineStartupPolicy,
    ) -> Self {
        self.surface(SurfaceMode::InlineTranscript {
            height: height.into(),
            startup,
        })
    }

    /// Configure mouse capture behavior.
    ///
    /// This sets the initial runtime state. Components can later change it with
    /// `Context::set_mouse_capture(...)` or `Context::toggle_mouse_capture()`.
    ///
    /// Defaults:
    /// - fullscreen mode: enabled
    /// - inline mode: disabled
    pub fn mouse(mut self, enabled: bool) -> Self {
        self.mouse_enabled = Some(enabled);
        self
    }

    /// Set the app-wide mouse wheel step multiplier.
    ///
    /// Each wheel tick scrolls `multiplier` lines instead of the default single
    /// line. Coalesced wheel bursts multiply by this value too, so two ticks with
    /// `multiplier = 3` scroll six lines total.
    pub fn scroll_wheel_multiplier(mut self, multiplier: u16) -> Self {
        self.scroll_wheel_multiplier = multiplier.max(1);
        self
    }

    /// Set the app-wide default theme.
    ///
    /// This theme is applied to the root tree every render.
    /// Use `ThemeProvider` to override a specific subtree.
    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Paint the root viewport background before rendering the UI tree.
    ///
    /// By default the framework paints nothing behind the tree (the host
    /// terminal background shows through). This opts into a filled background so
    /// the UI reads as a designed surface. Accepts a [`Color`], [`Paint`],
    /// [`Style`], or a [`ScreenBackground`] directly:
    ///
    /// ```no_run
    /// use tui_lipan::prelude::*;
    ///
    /// let app = App::new().screen_background(Color::hex_u24(0x04090D));
    /// ```
    ///
    /// Use [`fill_background`](Self::fill_background) to track the active theme's
    /// backdrop automatically.
    pub fn screen_background(mut self, background: impl Into<ScreenBackground>) -> Self {
        self.screen_background = background.into();
        self
    }

    /// Fill the root viewport with the active theme's backdrop surface.
    ///
    /// Shorthand for `screen_background(ScreenBackground::Theme)`. The fill tracks
    /// the app theme, so swapping themes keeps the backdrop in sync.
    pub fn fill_background(mut self) -> Self {
        self.screen_background = ScreenBackground::Theme;
        self
    }

    /// Set where toasts appear on screen.
    pub fn toast_placement(mut self, placement: ToastPlacement) -> Self {
        self.toast_placement = placement;
        self
    }

    /// Set vertical gap between stacked toasts.
    pub fn toast_gap(mut self, gap: u16) -> Self {
        self.toast_gap = gap;
        self
    }

    /// Set outside margin between toasts and the viewport edge.
    pub fn toast_margin(mut self, margin: impl Into<Padding>) -> Self {
        self.toast_margin = margin.into();
        self
    }

    /// Configure clipboard behavior.
    pub fn clipboard_config(mut self, config: ClipboardConfig) -> Self {
        self.clipboard_config = config;
        self
    }

    /// Use a specific keymap file path for this app instance.
    ///
    /// This path has higher priority than `TUI_LIPAN_KEYMAP` and the default
    /// `$XDG_CONFIG_HOME/tui-lipan/keymap.conf` fallback.
    pub fn keymap_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.keymap_path = Some(path.into());
        self
    }

    /// Override framework key bindings from Rust after file and built-in keymaps are loaded.
    pub fn framework_keymap(mut self, keymap: FrameworkKeymap) -> Self {
        self.framework_keymap = keymap;
        self
    }

    /// Configure the global quit shortcut. `None` disables framework quit bindings.
    pub fn global_quit(mut self, bindings: Option<KeyBindings>) -> Self {
        self.framework_keymap = match bindings {
            Some(bindings) => self.framework_keymap.bind(FrameworkAction::Quit, bindings),
            None => self.framework_keymap.unbind(FrameworkAction::Quit),
        };
        self
    }

    /// Enable or disable loading user keymap files.
    pub fn user_keymap_policy(mut self, policy: UserKeymapPolicy) -> Self {
        self.user_keymap_policy = policy;
        self
    }

    /// Configure app command versus widget key dispatch ordering.
    pub fn key_dispatch_policy(mut self, policy: KeyDispatchPolicy) -> Self {
        self.key_dispatch_policy = policy;
        self
    }

    /// Configure framework-initiated focus movement.
    pub fn focus_policy(mut self, policy: FocusPolicy) -> Self {
        self.focus_policy = policy;
        self
    }

    /// Observe completed focus transitions after widget blur/focus callbacks are emitted.
    pub fn on_focus_changed(mut self, hook: impl Fn(&FocusChanged) + 'static) -> Self {
        self.on_focus_changed = Some(std::rc::Rc::new(hook));
        self
    }

    /// Configure key dispatch behavior while terminal widgets are focused.
    pub fn terminal_key_policy(mut self, policy: TerminalKeyPolicy) -> Self {
        self.terminal_key_policy = policy;
        self
    }

    /// Configure how app command shortcut conflicts are resolved.
    pub fn command_conflict_policy(mut self, policy: CommandConflictPolicy) -> Self {
        self.command_conflict_policy = policy;
        self
    }

    /// Configure how mismatched keys are handled during pending command chords.
    pub fn chord_mismatch_policy(mut self, policy: ChordMismatchPolicy) -> Self {
        self.chord_mismatch_policy = policy;
        self
    }

    /// Configure which Enter key combination inserts new lines in `TextArea`.
    ///
    /// This policy is scoped to `TextArea` and does not change single-line
    /// `Input` behavior.
    pub fn text_area_newline_binding(mut self, binding: TextAreaNewlineBinding) -> Self {
        self.text_area_newline_binding = binding;
        self
    }

    /// Configure app-wide text contrast behavior for interactive widget states.
    ///
    /// Individual styles can override this per-state by setting
    /// `Style::contrast_policy(...)` on the relevant style (base, hover,
    /// selection, focus, theme role, etc.).
    pub fn contrast_policy(mut self, policy: ContrastPolicy) -> Self {
        self.contrast_policy = policy;
        self
    }

    /// Provide a custom clipboard provider implementation.
    pub fn clipboard_provider(mut self, provider: impl ClipboardProvider + 'static) -> Self {
        self.clipboard_provider = Some(Box::new(provider));
        self
    }

    /// Provide a custom clipboard error reporter.
    pub fn clipboard_reporter(mut self, reporter: impl Fn(ClipboardError) + 'static) -> Self {
        self.clipboard_reporter = std::rc::Rc::new(reporter);
        self
    }

    /// Set the resolved terminal background color.
    ///
    /// When set, [`crate::style::ColorTransform::Opacity`] can blend foreground colors
    /// toward the real terminal background even when a cell's background is
    /// [`Color::Reset`].  Obtain this value from [`crate::style::query_host_colors()`]
    /// before starting the app:
    ///
    /// ```ignore
    /// let bg = query_host_colors().map(|c| c.bg);
    /// App::new().terminal_bg(bg).run(MyComponent);
    /// ```
    pub fn terminal_bg(mut self, color: Option<Color>) -> Self {
        self.terminal_bg = color;
        self
    }

    /// Enable runner-managed host terminal color refreshes.
    ///
    /// When enabled, the runner queries the host terminal palette before component
    /// init, refreshes on terminal focus gained, and services
    /// `Context::request_host_terminal_color_refresh()` on the UI thread while
    /// coordinating with tui-lipan's input reader. Refreshed colors are exposed
    /// through `Context::host_terminal_colors()` and the resolved terminal
    /// background is kept in sync for opacity blending.
    ///
    /// On Unix fullscreen surfaces, compatible terminals that implement DEC
    /// private mode 2031 also trigger an immediate refresh when their palette
    /// changes. Inline, non-Unix, and unsupported terminals retain startup,
    /// focus-gained, and manual refresh behavior.
    ///
    /// Disabled by default so static apps do not poll the terminal.
    pub fn live_host_terminal_colors(mut self, enabled: bool) -> Self {
        self.live_host_terminal_colors = enabled;
        self
    }

    /// Use the host terminal palette as the app theme once colors are probed.
    ///
    /// The current app theme remains the fallback until the runner successfully
    /// receives host colors. Later failed refreshes keep the last applied theme.
    /// Unix fullscreen surfaces also subscribe to compatible terminals' DEC mode
    /// 2031 palette-change notifications while the app is running.
    pub fn system_theme(mut self) -> Self {
        self.system_theme = true;
        self
    }

    /// Configure runtime devtools subsystem behavior.
    #[cfg(feature = "devtools")]
    pub fn devtools_config(mut self, config: DevToolsConfig) -> Self {
        self.devtools_config = config;
        self
    }

    /// Mount the root component with default properties.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn mount<C>(self, component: C) -> AppRunner<C>
    where
        C: Component,
        C::Properties: Default,
    {
        self.mount_with_props(component, C::Properties::default())
    }

    /// Mount the root component with explicit properties.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn mount_with_props<C>(self, component: C, props: C::Properties) -> AppRunner<C>
    where
        C: Component,
    {
        AppRunner::new(self, component, props)
    }
}
