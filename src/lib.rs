#![deny(unsafe_code)]
#![warn(missing_docs)]
#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

//! tui-lipan: opinionated, component-based, modern TUI framework.
//!
//! This crate is built on top of `ratatui` + `crossterm` internally, but the
//! public API is backend-agnostic (no `ratatui` types leak).

#[cfg(all(target_arch = "wasm32", any(feature = "image", feature = "terminal")))]
compile_error!(
    "tui-lipan: wasm32 builds cannot enable image or terminal features; use --no-default-features"
);

pub mod prelude;

#[macro_use]
mod widget_manifest;

pub mod core;

pub mod animation;
pub mod capture;
pub mod debug;
#[cfg(feature = "devtools")]
pub(crate) mod devtools;
pub mod input;
#[cfg(not(target_arch = "wasm32"))]
pub mod process;
pub mod ui_snapshot;

mod clipboard;

mod app;
mod backend;
mod callback;
pub mod callbacks;
mod layout;
mod mockup;
mod overlay;
mod runtime;
mod ui;

pub mod style;

mod test_backend;
mod text;
pub mod text_motion;
pub mod utils;
pub mod validation;
mod widgets;

/// The host terminal's cell size in pixels.
///
/// A terminal deals in cells, but a program drawing a picture needs pixels: it reads the PTY's
/// `TIOCGWINSZ` pixel fields, or asks with `CSI 14 t`, and sizes its output against the answer.
/// Feed this to [`TerminalPtyConfig::cell_size`](crate::widgets::TerminalPtyConfig::cell_size) and
/// [`TerminalScreen::set_cell_size`](crate::widgets::TerminalScreen::set_cell_size) so the child's
/// arithmetic and the pane's layout agree.
///
/// Detected once at startup. Hosts that answer no size query get the image encoder's fallback
/// guess rather than a refusal, so this always names a usable cell.
#[cfg(all(feature = "terminal-images", not(target_arch = "wasm32")))]
pub fn host_cell_size() -> crate::widgets::TerminalCellSize {
    crate::backend::ratatui_backend::image_support::host_cell_size()
}

/// Why pointer reports are, or are not, pixel-precise on this host.
///
/// Mode 1016 needs two independent things to be true, and when it is off it is not obvious which
/// one is missing: the host has to answer the startup probe saying it implements the mode, and the
/// cell size has to be known exactly - from the size the host reports when asked, or from a window
/// size the grid divides evenly. A terminal that pads its window fails the second while passing the
/// first, and the mode then stays off silently, leaving reports cell-precise.
///
/// Reported rather than reasoned about, because both halves are answers from the host that no
/// amount of reading the code will produce.
/// Platforms without the mode report it inactive rather than making every caller spell out a
/// `cfg`: "pointer reports are cell-precise here" is the true answer, not a missing one.
#[cfg(not(target_arch = "wasm32"))]
pub fn pixel_pointer_status() -> PixelPointerStatus {
    #[cfg(unix)]
    {
        PixelPointerStatus {
            host_supports: crate::app::input::pixel_mouse::host_supports(),
            cell: crate::app::input::pixel_mouse::cell_size(),
        }
    }
    #[cfg(not(unix))]
    {
        PixelPointerStatus {
            host_supports: false,
            cell: None,
        }
    }
}

/// What [`pixel_pointer_status`] found. `active` is the conjunction, and is what actually decides
/// whether the mode is asked for and reports are read as pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(not(target_arch = "wasm32"))]
pub struct PixelPointerStatus {
    /// The host answered the `DECRQM` probe for mode 1016 saying it implements it.
    pub host_supports: bool,
    /// Cell size in pixels, when it is known exactly. `None` leaves the mode off whatever the host
    /// said, because a pixel report that cannot be divided into a cell is worse than a cell report.
    pub cell: Option<(u16, u16)>,
}

#[cfg(not(target_arch = "wasm32"))]
impl PixelPointerStatus {
    /// Whether pointer reports on this host carry pixels.
    pub fn active(&self) -> bool {
        self.host_supports && self.cell.is_some()
    }
}

/// Whether frames reach the host terminal through shared memory rather than through the PTY.
///
/// The difference is not a detail. A `t=s` transmission is a few hundred bytes naming an object the
/// host maps; the fallback deflates and base64s every pixel of every frame and writes the result
/// down the PTY, which for a full-window animation is tens of megabytes a second and shows up as
/// both CPU and visible stutter. Which one is in use is decided once, by whether the host answered
/// the startup probe, and is otherwise invisible - hence this.
///
/// Always false under `TMUX`, where the reader is tmux rather than a terminal on this machine.
#[cfg(all(feature = "terminal-images", not(target_arch = "wasm32")))]
pub fn host_reads_shared_frames() -> bool {
    crate::backend::ratatui_backend::shared_frame::host_reads_shared_memory()
        && std::env::var_os("TMUX").is_none()
}

/// Temporarily release the interactive terminal for an external program (e.g. `$EDITOR`).
///
/// Run suspend/resume on the **UI thread** (see [`Command::new`](crate::core::component::Command::new));
/// use [`Context::request_full_repaint`](crate::core::component::Context::request_full_repaint) after
/// return when a full frame redraw is needed. Human-oriented guide: `docs/external-programs.md` in the repo.
#[cfg(not(target_arch = "wasm32"))]
pub mod terminal_handoff {
    pub use crate::backend::ratatui_backend::terminal_handoff::{
        resume_after_external_process, suspend_for_external_process,
    };
}

#[cfg(not(target_arch = "wasm32"))]
pub use crate::app::AppRunner;
#[cfg(feature = "devtools")]
pub use crate::app::DevToolsConfig;
pub use crate::app::input::command_registry::{
    CommandBuilder, CommandEntry, CommandId, CommandRegistry,
};
pub use crate::app::input::key_dispatch::{
    ChordMismatchPolicy, CommandConflictPolicy, KeyDispatchPolicy, TerminalKeyPolicy,
};
pub use crate::app::input::keymap::{FrameworkAction, FrameworkKeymap, UserKeymapPolicy};
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub use crate::app::web_runner::{WebTerminal, mount_web};
pub use crate::app::{
    App, ContrastPolicy, DevToolsMetric, FocusChanged, FocusEntry, FocusPolicy, InlineHeight,
    InlineStartupPolicy, ScreenBackground, SurfaceMode, TextAreaNewlineBinding,
};
pub use crate::mockup::Mockup;

pub use crate::animation::{ExitAnimation, ExitQueue, ExitTransfer};
pub use crate::callback::{Callback, CancellationToken, CommandLink, KeyHandler, Link};
pub use crate::capture::{CapturedCell, CapturedFrame, CastRecording, CellModifiers, CursorState};
#[cfg(feature = "ui-snapshot-png")]
pub use crate::capture::{PngOptions, PngTextRenderer};
pub use crate::clipboard::{
    ClipboardConfig, ClipboardError, ClipboardHandle, ClipboardPasteContent, ClipboardProvider,
    ImageContent, ImageFormat, PasteShiftInsertBehavior,
};
pub use crate::core::component::{
    Breakpoint, Command, Component, Context, KeyUpdate, ScrollbarVisibility, TaskPolicy, Update,
    UpdateLevel,
};
pub use crate::core::context_value::ContextValue;
pub use crate::core::element::{Element, IntoElement, Key};
pub use crate::core::event::{
    KeyCode, KeyEvent, KeyMods, MouseDragEvent, MouseEvent, MouseMoveEvent,
};
pub use crate::core::mask::CellMask;
pub use crate::core::memo::Memo;
pub use crate::core::nested::any_props::ThemableProps;
pub use crate::core::node::NodeId;
pub use crate::input::{
    ChordMatcher, ChordResult, KeyBinding, KeyBindingParseError, KeyBindings,
    KeyEventExpansionError, format_binding, format_binding_compact, format_binding_lowercase,
    format_bindings, format_bindings_compact, format_bindings_lowercase,
};
pub use crate::layout::tag::Tag;
pub use crate::overlay::{OverlayId, OverlayScope, ToastHandle, ToastPlacement};
#[cfg(not(target_arch = "wasm32"))]
pub use crate::process::{
    ProcessEvent, ProcessExitStatus, ProcessSpec, process_command, process_command_keyed,
    stream_process, stream_process_until,
};
pub use crate::style::Theme;
pub use crate::style::{
    Align, BorderEdges, BorderStyle, CaretPalette, CaretShape, CellEffect, Color, ColorTransform,
    DiffPalette, DocumentPalette, DocumentViewPalette, Edge, EffectAxis, EffectCell, EffectContext,
    EffectPalette, EffectPrepareContext, FileIconPalette, FloatRect, GitStatusPalette,
    HexAreaPalette, HostTerminalColors, InputPalette, Justify, LayoutConstraints, Length, Padding,
    Paint, PreparedCellEffect, Rect, RetroPreset, RichText, RippleRadius, ScrollbarConfig,
    ScrollbarPalette, ScrollbarVariant, ShrinkPriority, Size, Span, StatusPalette, Style,
    SurfacePalette, SyntaxPalette, TerminalColor, TerminalPalette, TextAreaPalette, ThemeExtension,
    ThemePalette, VisualEffect, query_host_colors,
};
#[cfg(feature = "theme-reload")]
pub use crate::style::{ThemeWatcher, load_theme_from_toml};
pub use crate::test_backend::TestBackend;
pub use crate::text::edit::{TextEditEvent, TextEditKind};
pub use crate::text::editor::TextEditor;
pub use crate::text::line_index::{LineIndex, TextEncoding, TextPosition, TextRange};
pub use crate::ui_snapshot::{
    Action, FocusStep, Recording, ScrollDirection, Sketch, Target, UiSnapshot,
    UiSnapshotFileFormat, UiSnapshotFormatOptions, UiSnapshotOptions, UiSnapshotSlot, UiWidgetDesc,
    UiWidgetKind,
};
#[cfg(feature = "ui-snapshot-png")]
pub use crate::ui_snapshot::{BaselineComparison, BaselineOutcome, SnapshotBaseline};
pub use crate::validation::{StringValidator, ValidationError, Validator};
pub use crate::widgets::{Badge, BadgePosition, CapSides, CapStyle};
pub use crate::widgets::{
    BorderLabels, Canvas, CanvasItem, ClassDiagram, ClassDiagramTheme, ClassMember, ClassRelation,
    ClassRelationKind, ClassSpec, ClassVisibility, ContextProvider, DEFAULT_PREVIEW_MAX_HEIGHT,
    DEFAULT_PREVIEW_MAX_WIDTH, DiagramClassMemberSpec, DiagramClassNodeSpec,
    DiagramClassRelationSpec, DiagramClassSpec, DiagramClassVisibilitySpec, DiagramDirection,
    DiagramErAttributeSpec, DiagramErEntitySpec, DiagramErRelationSpec, DiagramErSpec,
    DiagramFlowEdgeSpec, DiagramFlowNodeShape, DiagramFlowNodeSpec, DiagramFlowchartSpec,
    DiagramGanttDate, DiagramGanttDuration, DiagramGanttSection, DiagramGanttSpec,
    DiagramGanttTask, DiagramGanttTaskStart, DiagramGanttTaskStatus, DiagramPieSliceSpec,
    DiagramPieSpec, DiagramSequenceMessageSpec, DiagramSequenceParticipantSpec,
    DiagramSequenceSpec, DiagramStateKindSpec, DiagramStateNodeSpec, DiagramStateSpec,
    DiagramStateTransitionSpec, DraggableTabBarOverflow, ErAttribute, ErCardinality, ErDiagram,
    ErDiagramTheme, ErEntity, ErRelation, FileKind, FileTree, FileTreeChange, FileTreeChangeSource,
    FileTreeChangeStatus, FileTreeChangeView, FileTreeDirectoryListing, FileTreeEntry,
    FileTreeEntryRequest, FileTreeEntrySource, FileTreeEvent, FileTreeExplorerFocusOrigin,
    FileTreeGitStatusCache, FileTreeGitView, FileTreeItemStyle, FileTreeSuffixPriority,
    FileTreeToggleEvent, FocusScope, FormattedDiagramBlock, Frame, FrameLabel, GanttDate,
    GanttDiagram, GanttDiagramTheme, GanttDuration, GanttSection, GanttSpec, GanttTask,
    GanttTaskStart, GanttTaskStatus, Heatmap, HeatmapCellMode, HeatmapLegendWidth, HexArea,
    HexAreaChangeEvent, HexAreaCursorEvent, HexAreaEditEvent, HexAreaEditKind, IMAGE_SENTINEL_BASE,
    PanEvent, PanKeymap, PanMetrics, PanView, ParsedDiagram, SENTINEL_BASE, ScrollAxis,
    ScrollBehavior, ScrollChildExitDirection, ScrollChildVisibility, ScrollDistanceConfig,
    ScrollEvent, ScrollExitedChild, ScrollMetrics, ScrollTarget, ScrollViewportEvent,
    ScrollVisibleChild, ScrollWheelBehavior, ScrollWheelConfig, SentinelEvent, SentinelId,
    StateDiagram, StateDiagramTheme, StateKind, StateSpec, StateTransition, TextArea,
    TextAreaColorInput, TextAreaColorLines, TextAreaColorStrategy, TextAreaCursorMetrics,
    TextAreaDecoration, TextAreaDecorationKind, TextAreaEvent, TextAreaGutter,
    TextAreaGutterColumn, TextAreaGutterSign, TextAreaImageMode, TextAreaLineNumberMode,
    TextAreaMetrics, TextAreaPasteEvent, TextAreaSentinel, TextAreaSentinelClickEvent,
    TextAreaSentinelClickKind, TextAreaSnapshot, TextAreaStateChangeEvent,
    TextAreaStateChangeReason, TextAreaVimConfig, TextAreaVimCurrentLineHighlight,
    TextAreaVimKeyBinding, TextAreaVimKeymap, TextAreaVimMode, TextAreaVirtualText, Toast,
    ToastCopyAffordance, TripleClickSelectionMode, VirtualTextPlacement, insert_sentinel,
    rank_search_palette_indices, rank_search_palette_indices_with_mode,
    rank_search_palette_indices_with_score,
};
#[cfg(feature = "qr-code")]
pub use crate::widgets::{QrCode, QrEcc, QrRender};

#[cfg(all(feature = "terminal", unix))]
pub use crate::widgets::TerminalPtyHandoff;
#[cfg(feature = "terminal")]
pub use crate::widgets::{
    CopyModeAction, CopyModeGrid, KittyKeyboardFlags, MouseEncoding, MouseMode, MouseModeState,
    TerminalCellSize, TerminalColorPalette, TerminalCopyMode, TerminalDecoration, TerminalKeyModes,
    TerminalPasteShortcutBehavior, TerminalRenderSnapshot, terminal_selection_text,
};
#[cfg(feature = "terminal-images")]
pub use crate::widgets::{
    GraphicsMediaPolicy, TerminalImage, TerminalImageCrop, TerminalImagePlacement,
};

#[cfg(feature = "diff-view")]
pub use crate::widgets::{
    DiffContextExpansion, DiffContextRange, DiffContextSeparatorDirection,
    DiffContextSeparatorEvent, DiffData, DiffDataConfig, DiffHunkAnchor, DiffInlineBlock,
    DiffLineAnchor, DiffLineClickEvent, DiffLineRange, DiffLineRangeEvent, DiffLineSide,
};

#[cfg(feature = "syntax-syntect")]
pub use crate::widgets::{
    SyntectDocumentFormatter, SyntectStrategy, apply_syntect_strategy_app_theme, language_from_path,
};
/// Macro for building `Element`s with struct-literal syntax.
pub use tui_lipan_macro::rsx;
/// Autocomplete-friendly macro for building `Element`s with builder chains.
///
/// Uses standard Rust builder syntax (full rust-analyzer autocomplete) with
/// `=> { children }` sugar for nesting.
pub use tui_lipan_macro::ui;

/// One-liner macro for previewing a TUI layout without any component boilerplate.
///
/// The body expression is automatically converted via `.into()`, so you can
/// return any widget builder directly (e.g. `Frame::new()...`) without calling
/// `.into()` yourself.  The closure uses `move` capture, so local data can be
/// referenced freely inside the body.
///
/// Press `Esc` or `q` to quit the preview.
///
/// # Basic usage
///
/// ```rust,no_run
/// use tui_lipan::prelude::*;
///
/// fn main() -> Result<()> {
///     mockup!("Dashboard", {
///         Frame::new()
///             .header_left("Panel")
///             .border(true)
///             .child(Text::new("Hello!"))
///     })
/// }
/// ```
///
/// # With sample data (mockup → app workflow)
///
/// Extract your UI into plain functions that return `Element`, then reuse them
/// in both mockup previews and real components:
///
/// ```rust,no_run
/// use tui_lipan::prelude::*;
///
/// fn sidebar(items: &[&str], sel: usize) -> Element {
///     Frame::new()
///         .header_left("Nav")
///         .border(true)
///         .child(List::new()
///             .items(items.iter().map(|s| ListItem::new(*s)))
///             .selected(sel))
///         .into()
/// }
///
/// fn main() -> Result<()> {
///     let items = vec!["Home", "Settings"];
///     mockup!("Preview", {
///         sidebar(&items, 0)
///     })
/// }
/// ```
#[cfg(not(target_arch = "wasm32"))]
#[macro_export]
macro_rules! mockup {
    ($title:expr, $body:expr) => {
        $crate::App::new()
            .title($title)
            .mount($crate::Mockup::new(move || { $body }.into()))
            .run()
    };
}

/// Create a nested component element.
///
/// This does not require `Component: Default`; pass a factory closure to construct the instance.
pub fn child<C, F>(factory: F, props: C::Properties) -> Element
where
    C: Component,
    F: Fn() -> C + 'static,
{
    Element::new(crate::core::element::ElementKind::Component(
        crate::core::nested::ComponentElement::new::<C, F>(factory, props),
    ))
}

/// Crate-wide result type.
pub type Result<T> = std::result::Result<T, Error>;

/// Crate-wide error type.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// I/O error.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// Syntax theme loading error.
    #[error("failed to load syntax theme `{name}`: {message}")]
    SyntaxThemeLoad {
        /// Theme name.
        name: String,
        /// Error details.
        message: String,
        /// Error message.
        #[source]
        error: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Internal message routing error.
    #[error(
        "message type mismatch for component `{component}` (expected `{expected}`, got `{actual}`)"
    )]
    MessageTypeMismatch {
        /// Type name of the component that received the message.
        component: &'static str,
        /// Expected message type name.
        expected: &'static str,
        /// Actual message type name.
        actual: &'static str,
    },

    /// Internal properties routing error.
    #[error(
        "props type mismatch for component `{component}` (expected `{expected}`, got `{actual}`)"
    )]
    PropsTypeMismatch {
        /// Type name of the component that received the props.
        component: &'static str,
        /// Expected props type name.
        expected: &'static str,
        /// Actual props type name.
        actual: &'static str,
    },

    /// Component expansion failure (props mismatch or mount failure during tree expansion).
    #[error("component expansion failed: {reason}")]
    ComponentExpansion {
        /// Human-readable description of the failure.
        reason: String,
    },

    /// Theme reload error.
    #[error("theme reload error: {message}")]
    ThemeReload {
        /// Error details.
        message: String,
    },
}

#[cfg(test)]
#[test]
fn inline_transcript_append_rejects_component_nodes() {
    crate::runtime::assert_inline_transcript_append_rejects_component_nodes();
}

#[cfg(test)]
#[test]
fn inline_surface_commit_render_path_is_unified() {
    crate::runtime::assert_inline_surface_commit_render_path_is_unified();
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[test]
fn inline_surface_internal_wrap_policy_is_opaque() {
    crate::backend::ratatui_backend::assert_inline_surface_internal_wrap_policy_is_opaque();
}
