//! This prelude is intentionally curated for app authors.
//!
//! It includes the most common types, widgets, and macros used to build
//! tui-lipan applications. For framework internals or less common widgets and
//! helpers, use explicit imports from `tui_lipan`.

// ─────────────────────────────────────────────────────────────────────────────
// Core component model
// ─────────────────────────────────────────────────────────────────────────────

pub use crate::core::component::{
    Breakpoint, Command, Component, Context, KeyUpdate, ScrollbarVisibility, TaskPolicy, Update,
};
pub use crate::core::element::{Element, IntoElement, Key};
pub use crate::core::event::{
    KeyCode, KeyEvent, KeyMods, MouseDragEvent, MouseEvent, MouseMoveEvent,
};
pub use crate::core::memo::Memo;

// ─────────────────────────────────────────────────────────────────────────────
// App runtime
// ─────────────────────────────────────────────────────────────────────────────

pub use crate::app::{
    App, ContrastPolicy, FocusPolicy, InlineHeight, InlineStartupPolicy, ScreenBackground,
    SurfaceMode, TextAreaNewlineBinding,
};

#[cfg(not(target_arch = "wasm32"))]
pub use crate::app::AppRunner;

#[cfg(feature = "devtools")]
pub use crate::app::DevToolsConfig;

// ─────────────────────────────────────────────────────────────────────────────
// Callbacks and messaging
// ─────────────────────────────────────────────────────────────────────────────

pub use crate::app::input::key_dispatch::{
    ChordMismatchPolicy, CommandConflictPolicy, KeyDispatchPolicy, TerminalKeyPolicy,
};
pub use crate::app::input::keymap::{FrameworkAction, FrameworkKeymap, UserKeymapPolicy};
pub use crate::callback::{Callback, CancellationToken, CommandLink, KeyHandler, Link};
pub use crate::{CommandBuilder, CommandEntry, CommandId, CommandRegistry};

// ─────────────────────────────────────────────────────────────────────────────
// Animation
// ─────────────────────────────────────────────────────────────────────────────

pub use crate::animation::{Easing, Transition, TransitionConfig};

// ─────────────────────────────────────────────────────────────────────────────
// Macros and helpers
// ─────────────────────────────────────────────────────────────────────────────

pub use crate::{Result, child, rsx, ui};

#[cfg(not(target_arch = "wasm32"))]
pub use crate::mockup;

pub use crate::mockup::Mockup;

// ─────────────────────────────────────────────────────────────────────────────
// Overlays
// ─────────────────────────────────────────────────────────────────────────────

pub use crate::overlay::{OverlayId, OverlayScope, ToastHandle, ToastPlacement};

#[cfg(not(target_arch = "wasm32"))]
pub use crate::{
    ProcessEvent, ProcessExitStatus, ProcessSpec, process_command, process_command_keyed,
    stream_process, stream_process_until,
};

// ─────────────────────────────────────────────────────────────────────────────
// Styling
// ─────────────────────────────────────────────────────────────────────────────

pub use crate::style::Theme;
pub use crate::style::query_host_colors;
pub use crate::style::{
    Align, BorderEdges, BorderStyle, CaretShape, CellEffect, Color, ColorTransform, Edge,
    EffectAlignment, EffectCell, EffectContext, EffectOrigin, EffectPrepareContext, FloatRect,
    HostTerminalColors, Justify, LayoutConstraints, Length, Padding, Paint, PreparedCellEffect,
    Rect, RetroPreset, RichText, RippleRadius, ScrollbarConfig, ScrollbarVariant, ShrinkPriority,
    Size, Span, Style, TerminalColor, VisualEffect,
};

// ─────────────────────────────────────────────────────────────────────────────
// Utilities and validation
// ─────────────────────────────────────────────────────────────────────────────

pub use crate::utils::gradient::{ColorGradient, GradientDirection, GradientRange};
pub use crate::validation::{StringValidator, ValidationError, Validator};

// ─────────────────────────────────────────────────────────────────────────────
// Text editing
// ─────────────────────────────────────────────────────────────────────────────

pub use crate::text::edit::{TextEditEvent, TextEditKind};
pub use crate::text::editor::TextEditor;
pub use crate::text::input::TextInput;
pub use crate::text::line_index::{LineIndex, TextEncoding, TextPosition, TextRange};
pub use crate::text_motion::{
    big_word_backward_start, big_word_end, big_word_forward_start, first_nonblank_in_line,
    line_end_at, line_start_at, word_backward_start, word_end, word_forward_start,
};

// ─────────────────────────────────────────────────────────────────────────────
// Keybindings
// ─────────────────────────────────────────────────────────────────────────────

pub use crate::input::{KeyBinding, KeyBindings};

// ─────────────────────────────────────────────────────────────────────────────
// Clipboard
// ─────────────────────────────────────────────────────────────────────────────

pub use crate::clipboard::{ClipboardConfig, PasteShiftInsertBehavior};

// ─────────────────────────────────────────────────────────────────────────────
// Curated widgets
// ─────────────────────────────────────────────────────────────────────────────

pub use crate::widgets::{
    Accordion, AccordionItem, ActivationTheme, ActorKind, ActorRef, Animated, AnimationFrame,
    AsciiCanvas, AsciiCanvasBuffer, AsciiCell, AutonumberTheme, Badge, BadgePosition,
    BorderMergeMode, Breadcrumb, Button, ButtonVariant, Canvas, CanvasItem, Center, CenterPin,
    Chart, ChartAxis, ChartSeries, ChartSeriesMode, ChartThreshold, Checkbox, CheckboxEvent,
    CheckboxState, CheckboxVariant, ClassDiagram, ClassDiagramTheme, ClassMember, ClassRelation,
    ClassRelationKind, ClassSpec, ClassVisibility, ColumnAlign, ColumnWidth, ComboBox,
    ComboBoxCommitEvent, CommandPalette, ContentFormatter, ContextMenu, ContextProvider, DateEvent,
    DatePicker, DecorationGlyph, DecorationPlacement, DescriptionOverflow, DescriptionPlacement,
    DiagramClassMemberSpec, DiagramClassNodeSpec, DiagramClassRelationSpec, DiagramClassSpec,
    DiagramClassVisibilitySpec, DiagramDirection, DiagramErAttributeSpec, DiagramErEntitySpec,
    DiagramErRelationSpec, DiagramErSpec, DiagramFlowEdgeSpec, DiagramFlowNodeShape,
    DiagramFlowNodeSpec, DiagramFlowchartSpec, DiagramGanttDate, DiagramGanttDuration,
    DiagramGanttSection, DiagramGanttSpec, DiagramGanttTask, DiagramGanttTaskStart,
    DiagramGanttTaskStatus, DiagramPieSliceSpec, DiagramPieSpec, DiagramSequenceMessageSpec,
    DiagramSequenceParticipantSpec, DiagramSequenceSpec, DiagramStateKindSpec,
    DiagramStateNodeSpec, DiagramStateSpec, DiagramStateTransitionSpec, Divider,
    DocumentClickEvent, DocumentLineNumberMode, DocumentScrollMetrics, DocumentSelectEvent,
    DocumentStyles, DocumentTableWidthMode, DocumentView, DragCancelEvent, DragLeaveEvent,
    DragOverEvent, DragPayload, DragPreview, DragReorderMode, DragSlot, DragSlotAxis, DragSource,
    DragStartEvent, DragStartedEvent, DraggableTab, DraggableTabActionEvent, DraggableTabBar,
    DraggableTabBarOverflow, DraggableTabBarVariant, DraggableTabCloseEvent, DraggableTabHitPart,
    DraggableTabKind, DraggableTabReorderEvent, DraggableTabTransferEvent, DropEvent,
    DropHighlight, DropSlot, DropTarget, Edge as FlowchartEdge, EdgeArrow, EdgeDecoration,
    EdgeStyle, EffectScope, ErAttribute, ErCardinality, ErDiagram, ErDiagramTheme, ErEntity,
    ErRelation, FileIconStyle, FileKind, FileTree, FileTreeChange, FileTreeChangeSource,
    FileTreeChangeStatus, FileTreeChangeView, FileTreeEvent, FileTreeGitView, FileTreeItemStyle,
    FileTreeSuffixPriority, FileTreeToggleEvent, Flow, FlowDirection, Flowchart,
    FlowchartEdgeEvent, FlowchartItemPath, FlowchartNodeEvent, FlowchartSubgraphEvent,
    FlowchartTheme, FocusAccordion, FocusSizing, FormatInput, FormattedBlock,
    FormattedDiagramBlock, FormattedDocument, FormattedLine, FragmentGlyphs, FragmentKind, Frame,
    FrameParseError, FrameSequence, FrameSequenceBuilder, GanttDate, GanttDiagram,
    GanttDiagramTheme, GanttDuration, GanttSection, GanttSpec, GanttTask, GanttTaskStart,
    GanttTaskStatus, GitChangeState, GitFileStatus, GitIconStyle, Graph, GraphDirection,
    GraphLayout, GraphNode, GraphNodeEvent, GraphNodePath, Grid, GridItem, GridProps, HStack,
    Hyperlink, HyperlinkEvent, IndentStyle, Input, InputEvent, ItemDescription, LifelineTheme,
    List, ListConfig, ListEvent, ListItem, ListItemGutter, ListItemLine, ListItemRole,
    ListItemStatus, ListSymbolPosition, LogBuffer, LogEntry, LogFilterMode, LogLevel, LogView,
    LogViewEvent, MessageGlyphs, MessageStyle, Modal, MouseRegion, Msg, MultiSelect,
    MultiSelectChangeEvent, MultiSelectCommitEvent, MultiSelectDescriptionOverflow,
    MultiSelectDescriptionPlacement, MultiSelectItem, MultiSelectToggleEvent, NodeId, NodeShape,
    NotePlacement, Orientation, Overflow, PaginationAction, PaginationBar,
    PaginationButtonOverrides, PaginationInfo, PaginationLabels, PaginationState, PanEvent,
    PanKeymap, PanMetrics, PanView, ParsedDiagram, PlainFormatter, Popover, PopoverOffset,
    PopoverPlacement, ProgressBar, ProgressEvent, ProgressStyle, ProgressTextPosition,
    ProgressZone, Radio, RadioLayout, ScrollAxis, ScrollBehavior, ScrollChildExitDirection,
    ScrollChildVisibility, ScrollClip, ScrollDistanceConfig, ScrollEvent, ScrollExitedChild,
    ScrollKeymap, ScrollMetrics, ScrollRequest, ScrollTarget, ScrollView, ScrollViewportEvent,
    ScrollVisibleChild, ScrollWheelBehavior, ScrollWheelConfig, SearchEntry, SearchEvent,
    SearchHighlight, SearchItem, SearchMatchMode, SearchPalette, Select, SequenceDiagram,
    SequenceDiagramTheme, SequenceDiagramVariant, SequenceItemEvent, SequenceItemPath,
    SequenceMessage, SequenceStep, Slider, Spacer, Sparkline, SparklineAggregation,
    SparklineBarsPreset, SparklineLineGlyphs, SparklineLinePreset, SparklineVariant,
    SparklineZeroPolicy, Spinner, SpinnerSpeed, SpinnerStyle, Splitter, SplitterHandleMode,
    SplitterResizeEvent, StateDiagram, StateDiagramTheme, StateKind, StateSpec, StateTransition,
    StatusBar, Step, Tab, TabVariant, Table, TableCell, TableDisclosureState, TableEvent, TableRow,
    TableRowRole, TableRowSeparators, Tabs, TabsEvent, TabsOverflow, Text, TextArea,
    TextAreaCursorMetrics, TextAreaDecoration, TextAreaDecorationKind, TextAreaEvent,
    TextAreaGutter, TextAreaGutterColumn, TextAreaGutterSign, TextAreaLineNumberMode,
    TextAreaMetrics, TextAreaPasteEvent, TextAreaStateChangeEvent, TextAreaStateChangeReason,
    TextAreaVimConfig, TextAreaVimCurrentLineHighlight, TextAreaVimKeyBinding, TextAreaVimKeymap,
    TextAreaVimMode, TextAreaVirtualText, ThemeProvider, Toast, ToastCopyAffordance, Tooltip, Tree,
    TreeEvent, TreeKeymap, TreeNode, TreePath, TreeToggleEvent, VStack, VirtualTextPlacement,
    ZStack,
};

// ─────────────────────────────────────────────────────────────────────────────
// Feature-gated curated widgets
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(feature = "big-text")]
pub use crate::widgets::{BigFont, BigText, GlyphLayout, Shadow};

#[cfg(feature = "diff-view")]
pub use crate::widgets::{
    DiffContextExpansion, DiffContextRange, DiffContextSeparatorDirection,
    DiffContextSeparatorEvent, DiffData, DiffDataConfig, DiffHunkAnchor, DiffPane, DiffPrefixes,
    DiffScrollEvent, DiffView, DiffViewBackend, DiffViewMode,
};

#[cfg(feature = "image")]
pub use crate::widgets::{Image, ImageFit, ImagePlayback, ImageProtocol, ImageRepeat, ImageSource};

#[cfg(feature = "markdown")]
pub use crate::widgets::MarkdownFormatter;

#[cfg(feature = "syntax-syntect")]
pub use crate::widgets::{
    SyntectDocumentFormatter, SyntectStrategy, apply_syntect_strategy_app_theme, language_from_path,
};

#[cfg(all(feature = "terminal", unix))]
pub use crate::widgets::TerminalPtyHandoff;
#[cfg(feature = "terminal")]
pub use crate::widgets::{
    KittyKeyboardFlags, ManagedTerminal, ManagedTerminalProps, ManagedTerminalStatus,
    MouseEncoding, MouseMode, MouseModeState, Terminal, TerminalBuffer, TerminalColorPalette,
    TerminalCommandPhase, TerminalInputEvent, TerminalInputKind, TerminalKeyModes, TerminalPty,
    TerminalPtyConfig, TerminalPtyError, TerminalPtyEvent, TerminalRenderSnapshot, TerminalScreen,
    TerminalSelection, TerminalSelectionEvent, TerminalSemanticEvent, TerminalSemanticState,
    TerminalViewport, TerminalWorkingDirectory, TerminalWorkingDirectorySource, encode_paste,
    focus_sequences, key_event_to_bytes, mouse_event_to_bytes, paste_sequences,
};

// ─────────────────────────────────────────────────────────────────────────────
// Feature-gated utilities
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(feature = "theme-reload")]
pub use crate::{ThemeWatcher, load_theme_from_toml};
