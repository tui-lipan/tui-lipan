//! Curated prelude for `tui-lipan` application modules.
//!
//! Includes what an ordinary app module names regularly: the component model,
//! `App` and its common builder options, keyboard/mouse events, core styling
//! and animation types, macros, and every widget in `docs/widgets/index.md`
//! with the event, item, and enum types required by their common builders.
//!
//! Specialized and lower-level APIs live at the crate root (`tui_lipan::*`)
//! instead of here - including automation/testing, terminal protocol and image
//! internals, text-motion helpers, diagram spec/theme sub-types, `CursorTrail`
//! sub-types, custom `CellEffect` internals, `FileTree` git/cache internals,
//! and advanced `DiffView`/`TextArea` configuration types.

// ─────────────────────────────────────────────────────────────────────────────
// Core component model & callbacks
// Rule: types named when defining Components, state/messages, callbacks, and Element trees.
// ─────────────────────────────────────────────────────────────────────────────

pub use crate::callback::{Callback, CommandLink, KeyHandler, Link};
pub use crate::core::component::{
    Breakpoint, Command, Component, Context, KeyUpdate, TaskPolicy, Update,
};
pub use crate::core::element::{Element, IntoElement, Key};
pub use crate::core::memo::Memo;
pub use crate::core::node::NodeId;

// ─────────────────────────────────────────────────────────────────────────────
// Macros & entry helpers
// Rule: declarative UI macros, child component mounting, mockup helpers, and Result.
// ─────────────────────────────────────────────────────────────────────────────

pub use crate::mockup::Mockup;
pub use crate::{Result, child, rsx, ui};

#[cfg(not(target_arch = "wasm32"))]
pub use crate::mockup;

// ─────────────────────────────────────────────────────────────────────────────
// App runtime & common configuration
// Rule: App and the primary option types passed to common App builders and commands.
// Sub-types (CursorTrail*, Clipboard*, keymap dispatch policies) live at the crate root.
// ─────────────────────────────────────────────────────────────────────────────

pub use crate::app::{
    App, ContrastPolicy, CursorTrail, FocusPolicy, InlineHeight, ScreenBackground,
    TextAreaNewlineBinding,
};
pub use crate::clipboard::ClipboardConfig;
pub use crate::input::{KeyBinding, KeyBindings};
pub use crate::overlay::{OverlayScope, ToastPlacement};
pub use crate::{CommandEntry, CommandRegistry};

#[cfg(feature = "devtools")]
pub use crate::app::DevToolsConfig;

// ─────────────────────────────────────────────────────────────────────────────
// Input events
// Rule: primary keyboard and mouse events handled by components and MouseRegion.
// ─────────────────────────────────────────────────────────────────────────────

pub use crate::core::event::{
    KeyCode, KeyEvent, KeyMods, MouseDragEvent, MouseEvent, MouseMoveEvent, MouseRegionEvent,
};

// ─────────────────────────────────────────────────────────────────────────────
// Core styling, layout, effects & animation
// Rule: everyday styling, sizing, theme, rich-text, gradient, and common effect/transition types.
// Custom CellEffect trait internals and low-level palette/bezier types live at the crate root.
// ─────────────────────────────────────────────────────────────────────────────

pub use crate::animation::{Easing, TransitionConfig, VisibilityAnimation};
pub use crate::style::{
    Align, BorderEdges, BorderStyle, Color, ColorTransform, Edge, EffectOrigin, Justify, Length,
    Padding, Paint, Rect, RetroPreset, RichText, RippleRadius, ScrollbarConfig, ScrollbarVariant,
    Size, Span, Style, Theme, VisualEffect, query_host_colors,
};
pub use crate::utils::gradient::{ColorGradient, GradientDirection};

// ─────────────────────────────────────────────────────────────────────────────
// Text editing state
// Rule: backing state structs for Input and TextArea. Text-motion helpers live at the crate root.
// ─────────────────────────────────────────────────────────────────────────────

pub use crate::text::editor::TextEditor;
pub use crate::text::input::TextInput;

// ─────────────────────────────────────────────────────────────────────────────
// Widgets & common builder types
// Rule: every widget listed in docs/widgets/index.md plus the event, item, and enum types
// needed by its common builders. Theme/spec/git/cache/internal sub-types live at the crate root.
// ─────────────────────────────────────────────────────────────────────────────

pub use crate::widgets::{
    Accordion, AccordionItem, Animated, AsciiCanvas, Badge, BadgePosition, BorderLabels,
    BorderMergeMode, Breadcrumb, Button, ButtonVariant, Canvas, Center, CenterPin, Chart,
    ChartAxis, ChartSeries, ChartSeriesMode, Checkbox, CheckboxEvent, CheckboxState, ClassDiagram,
    ClassRelationKind, ClassVisibility, ColumnWidth, ComboBox, ComboBoxCommitEvent, CommandPalette,
    ContextMenu, ContextProvider, DateEvent, DatePicker, Divider, DocumentClickEvent, DocumentView,
    DragPayload, DragSource, DraggableTab, DraggableTabBar, DraggableTabCloseEvent,
    DraggableTabReorderEvent, DraggableTabTransferEvent, DropEvent, DropTarget, EffectScope,
    ErCardinality, ErDiagram, FileIconStyle, FileTree, FileTreeEntry, FileTreeEntrySource,
    FileTreeEvent, Flow, FlowDirection, Flowchart, FlowchartEdge, Frame, FrameSequence,
    GanttDiagram, GanttSection, GanttTask, Graph, GraphDirection, GraphNode, GraphNodeEvent, Grid,
    HStack, Heatmap, HexArea, Hyperlink, HyperlinkEvent, IndentStyle, Input, InputEvent,
    KeyCapture, List, ListEvent, ListItem, LogEntry, LogLevel, LogView, LogViewEvent, Modal,
    MouseRegion, MultiSelect, MultiSelectChangeEvent, MultiSelectCommitEvent, MultiSelectItem,
    NodeShape, Orientation, Overflow, PaginationAction, PaginationBar, PaginationState, PanEvent,
    PanView, Popover, PopoverPlacement, ProgressBar, ProgressEvent, ProgressStyle,
    ProgressTextPosition, Radio, RadioLayout, ScrollEvent, ScrollKeymap, ScrollView, SearchEvent,
    SearchItem, SearchPalette, Select, SequenceDiagram, SequenceMessage, Slider, Spacer, Sparkline,
    SparklineVariant, Spinner, SpinnerSpeed, SpinnerStyle, Splitter, SplitterResizeEvent,
    StateDiagram, StatusBar, Tab, Table, TableCell, TableEvent, TableRow, Tabs, TabsEvent, Text,
    TextArea, TextAreaEvent, TextAreaLineNumberMode, ThemeProvider, Toast, Tooltip, Tree,
    TreeEvent, TreeExpansionMode, TreeNode, TreeToggleEvent, VStack, ZStack,
};

// ─────────────────────────────────────────────────────────────────────────────
// Feature-gated widgets & primary builder types
// Rule: widgets enabled by optional Cargo features plus their primary events/configs.
// Terminal protocol, history, damage, and graphics internals live at the crate root.
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(feature = "big-text")]
pub use crate::widgets::{BigFont, BigText};

#[cfg(feature = "diff-view")]
pub use crate::widgets::{DiffPane, DiffView, DiffViewBackend, DiffViewMode};

#[cfg(feature = "image")]
pub use crate::widgets::{Image, ImageFit};

#[cfg(feature = "markdown")]
pub use crate::widgets::MarkdownFormatter;

#[cfg(feature = "qr-code")]
pub use crate::widgets::QrCode;

#[cfg(feature = "syntax-syntect")]
pub use crate::widgets::SyntectStrategy;

#[cfg(feature = "terminal")]
pub use crate::widgets::{
    ManagedTerminal, ManagedTerminalStatus, Terminal, TerminalInputEvent, TerminalLinkEvent,
    TerminalPty, TerminalPtyConfig, TerminalPtyEvent, TerminalRenderSnapshot, TerminalScreen,
    TerminalSelectionEvent, TerminalViewport,
};
