//! Widget definitions for tui-lipan.
//!
//! There are two types of widgets in tui-lipan: **primitives** and **composites**.
//!
//! # Primitive Widgets
//!
//! Primitives are the fundamental building blocks (like `Button`, `VStack`, `Text`).
//! They are implemented as directory modules containing:
//! - `mod.rs`: The public API (builder struct).
//! - `node.rs`: The internal render node (`WidgetNode` implementation).
//! - `layout.rs`: Layout logic (sizing and positioning).
//! - `reconcile.rs`: Diffing logic for updating the node.
//!
//! Example: `src/widgets/button/`
//!
//! # Composite Widgets
//!
//! Composites are higher-level widgets built by combining existing primitives.
//! They are typically implemented as single files that return a tree of other widgets.
//! They do not implement `WidgetNode` directly but instead return an `Element` (like a Component).
//!
//! Example: `src/widgets/badge.rs` (combines `Text` and `VStack` with specific styling).

mod accordion;
mod animated;
mod ascii_canvas;
mod badge;
#[cfg(feature = "big-text")]
mod big_text;
mod breadcrumb;
mod button;
mod canvas;
mod center;
mod center_pin;
mod chart;
mod checkbox;
mod class_diagram;
mod combo_box;
mod command_palette;
pub(crate) mod common;
pub(crate) mod containers;
mod context_menu;
mod context_provider;
mod date_picker;
#[cfg(feature = "diff-view")]
mod diff_view;
mod divider;
pub(crate) mod document_view;
mod drag_drop;
pub(crate) mod draggable_tab_bar;
mod effect_scope;
mod er_diagram;
mod file_tree;
mod flow;
mod flowchart;
mod frame;
mod gantt_diagram;
mod graph;
mod grid;
mod heatmap;
mod hex_area;
mod hyperlink;
#[cfg(feature = "image")]
mod image;
mod input;
pub(crate) mod list;
pub(crate) mod log_view;
#[cfg(feature = "terminal")]
mod managed_terminal;
mod modal;
mod mouse_region;
mod multi_select;
mod pagination;
mod pan_view;
mod popover;
mod progress;
mod radio;
pub(crate) mod scroll;
mod scroll_view;
pub(crate) mod search_palette;
mod select;
mod selection;
mod sequence_diagram;
pub(crate) mod slider;
mod spacer;
mod sparkline;
mod spinner;
mod splitter;
mod state_diagram;
mod status_bar;
pub(crate) mod status_bar_layout;
pub(crate) mod table;
mod tabs;
#[cfg(feature = "terminal")]
mod terminal;
mod text;
mod text_area;
pub(crate) mod theme_provider;
pub(crate) mod toast;
mod tooltip;
mod tree;
mod zstack;

pub(crate) use scroll_view::node::{OffscreenDocSelection, SingleDocSelection};
pub(crate) use scroll_view::utils::{
    calc_scroll_view_window, normalize_input_offset, scroll_view_scrollbar_metrics,
};
pub(crate) use status_bar_layout::StatusBarLayout;
pub(crate) use text_area::TextAreaGeometry;
pub(crate) use text_area::text_area_cursor_reserve;
pub(crate) use text_area::text_area_visual_line_for_cursor;

// Public re-exports.
pub use accordion::{Accordion, AccordionItem};
pub use animated::Animated;
pub use ascii_canvas::animation::{
    AnimationFrame, FrameParseError, FrameSequence, FrameSequenceBuilder,
};
pub use ascii_canvas::{AsciiCanvas, AsciiCanvasBuffer, AsciiCell};
pub use badge::{Badge, BadgePosition};
#[cfg(feature = "big-text")]
pub use big_text::{BigFont, BigText, GlyphLayout, Shadow};
pub use breadcrumb::Breadcrumb;
pub use button::{Button, ButtonVariant};
pub use canvas::{Canvas, CanvasItem};
pub use center::Center;
pub use center_pin::CenterPin;
pub use chart::{Chart, ChartAxis, ChartSeries, ChartSeriesMode, ChartThreshold};
pub use checkbox::{Checkbox, CheckboxEvent, CheckboxState, CheckboxVariant};
pub use class_diagram::{
    ClassDiagram, ClassDiagramTheme, ClassMember, ClassRelation, ClassRelationKind, ClassSpec,
    ClassVisibility,
};
pub use combo_box::{ComboBox, ComboBoxCommitEvent};
pub use command_palette::CommandPalette;
pub use containers::{FocusAccordion, FocusPolicy, HStack, TabVariant, VStack};
pub use context_menu::ContextMenu;
pub use context_provider::ContextProvider;
pub use date_picker::{DateEvent, DatePicker};
#[cfg(feature = "diff-view")]
pub(crate) use diff_view::{
    DiffColorStrategy, DiffContextSeparatorClickConfig, DiffDocumentFormatter, SplitWrapDualPass,
    element_subtree_has_split_wrap_sync, rebuild_diff_gutter_spans, split_wrap_layout_pass,
    split_wrap_pane_widths, split_wrap_scrollbar_cols_pair,
};
#[cfg(feature = "diff-view")]
pub use diff_view::{
    DiffContextExpansion, DiffContextRange, DiffContextSeparatorDirection,
    DiffContextSeparatorEvent, DiffData, DiffDataConfig, DiffHunkAnchor, DiffPane, DiffPrefixes,
    DiffScrollEvent, DiffView, DiffViewBackend, DiffViewMode,
};
pub use divider::{Divider, Orientation};
#[cfg(feature = "markdown")]
pub use document_view::MarkdownFormatter;
pub use document_view::diagram::{
    ClassMemberSpec as DiagramClassMemberSpec, ClassNodeSpec as DiagramClassNodeSpec,
    ClassRelationSpec as DiagramClassRelationSpec, ClassSpec as DiagramClassSpec,
    ClassVisibilitySpec as DiagramClassVisibilitySpec, DiagramDirection,
    ErAttributeSpec as DiagramErAttributeSpec, ErEntitySpec as DiagramErEntitySpec,
    ErRelationSpec as DiagramErRelationSpec, ErSpec as DiagramErSpec,
    FlowEdgeSpec as DiagramFlowEdgeSpec, FlowNodeShape as DiagramFlowNodeShape,
    FlowNodeSpec as DiagramFlowNodeSpec, FlowchartSpec as DiagramFlowchartSpec, ParsedDiagram,
    PieSliceSpec as DiagramPieSliceSpec, PieSpec as DiagramPieSpec,
    SequenceMessageSpec as DiagramSequenceMessageSpec,
    SequenceParticipantSpec as DiagramSequenceParticipantSpec, SequenceSpec as DiagramSequenceSpec,
    StateKindSpec as DiagramStateKindSpec, StateNodeSpec as DiagramStateNodeSpec,
    StateSpec as DiagramStateSpec, StateTransitionSpec as DiagramStateTransitionSpec,
};
pub use document_view::{
    ColumnAlign, ContentFormatter, DocumentClickEvent, DocumentLineNumberMode,
    DocumentScrollMetrics, DocumentSelectEvent, DocumentStyles, DocumentTableWidthMode,
    DocumentView, FormatInput, FormattedBlock, FormattedDiagramBlock, FormattedDocument,
    FormattedLine, PlainFormatter, TableRowSeparators,
};
pub use drag_drop::{
    DEFAULT_PREVIEW_MAX_HEIGHT, DEFAULT_PREVIEW_MAX_WIDTH, DragCancelEvent, DragLeaveEvent,
    DragOverEvent, DragPayload, DragPreview, DragSlot, DragSlotAxis, DragSource, DragStartEvent,
    DragStartedEvent, DropEvent, DropHighlight, DropSlot, DropTarget,
};
pub use draggable_tab_bar::{
    DragReorderMode, DraggableTab, DraggableTabActionEvent, DraggableTabBar,
    DraggableTabBarOverflow, DraggableTabBarVariant, DraggableTabCloseEvent, DraggableTabHitPart,
    DraggableTabKind, DraggableTabReorderEvent, DraggableTabTransferEvent,
};
pub use effect_scope::EffectScope;
pub use er_diagram::{ErAttribute, ErCardinality, ErDiagram, ErDiagramTheme, ErEntity, ErRelation};
pub use file_tree::{
    FileIconStyle, FileKind, FileTree, FileTreeChange, FileTreeChangeSource, FileTreeChangeStatus,
    FileTreeChangeView, FileTreeEvent, FileTreeGitView, FileTreeItemStyle, FileTreeSuffixPriority,
    FileTreeToggleEvent, GitChangeState, GitFileStatus, GitIconStyle,
};
pub use flow::Flow;
pub use flowchart::{
    Edge, EdgeArrow, EdgeStyle, FlowDirection, Flowchart, FlowchartEdgeEvent, FlowchartItemPath,
    FlowchartNodeEvent, FlowchartSubgraphEvent, FlowchartTheme, NodeId, NodeShape,
};
pub use frame::{BorderMergeMode, DecorationGlyph, DecorationPlacement, EdgeDecoration, Frame};
pub use gantt_diagram::{
    GanttDate, GanttDate as DiagramGanttDate, GanttDiagram, GanttDiagramTheme, GanttDuration,
    GanttDuration as DiagramGanttDuration, GanttRenderRole, GanttSection,
    GanttSection as DiagramGanttSection, GanttSpec, GanttSpec as DiagramGanttSpec, GanttTask,
    GanttTask as DiagramGanttTask, GanttTaskStart, GanttTaskStart as DiagramGanttTaskStart,
    GanttTaskStatus, GanttTaskStatus as DiagramGanttTaskStatus,
};
pub use graph::{Graph, GraphDirection, GraphLayout, GraphNode, GraphNodeEvent, GraphNodePath};
pub use grid::{Grid, GridItem, GridProps};
pub use heatmap::{Heatmap, HeatmapCellMode, HeatmapLegendWidth};
pub use hex_area::{
    HexArea, HexAreaChangeEvent, HexAreaCursorEvent, HexAreaEditEvent, HexAreaEditKind,
};
pub(crate) use hex_area::{HexAreaPointerHitArgs, pointer_hit};
pub use hyperlink::{Hyperlink, HyperlinkEvent};
#[cfg(feature = "image")]
pub use image::{Image, ImageFit, ImagePlayback, ImageProtocol, ImageRepeat, ImageSource};
pub use input::{Input, InputEvent};
pub use list::{
    List, ListConfig, ListEvent, ListItem, ListItemGutter, ListItemLine, ListItemRole,
    ListItemStatus, ListSymbolPosition,
};
pub use log_view::{LogBuffer, LogEntry, LogFilterMode, LogLevel, LogView, LogViewEvent};
#[cfg(feature = "terminal")]
pub use managed_terminal::{ManagedTerminal, ManagedTerminalProps, ManagedTerminalStatus};
pub use modal::Modal;
pub use mouse_region::MouseRegion;
pub use multi_select::{
    MultiSelect, MultiSelectChangeEvent, MultiSelectCommitEvent, MultiSelectDescriptionOverflow,
    MultiSelectDescriptionPlacement, MultiSelectItem, MultiSelectToggleEvent,
};
pub use pagination::{
    PaginationAction, PaginationBar, PaginationButtonOverrides, PaginationInfo, PaginationLabels,
    PaginationState,
};
pub use pan_view::{PanEvent, PanKeymap, PanMetrics, PanView};
pub use popover::{Popover, PopoverOffset, PopoverPlacement};
pub use progress::{ProgressBar, ProgressEvent, ProgressStyle, ProgressTextPosition, ProgressZone};
pub use radio::{Radio, RadioLayout};
pub use scroll::ScrollAxis;
pub(crate) use scroll_view::node::{
    HeightCacheItem, ScrollViewLayoutCache, VirtualChildEntry, VirtualHeightCache,
};
pub(crate) use scroll_view::reconcile::scroll_child_height_depends_on_width;
pub use scroll_view::{
    ScrollBehavior, ScrollChildExitDirection, ScrollChildVisibility, ScrollClip,
    ScrollDistanceConfig, ScrollEvent, ScrollExitedChild, ScrollKeymap, ScrollMetrics,
    ScrollRequest, ScrollTarget, ScrollView, ScrollViewportEvent, ScrollVisibleChild,
    ScrollWheelBehavior, ScrollWheelConfig,
};
pub use search_palette::{
    DescriptionOverflow, DescriptionPlacement, ItemDescription, SearchEntry, SearchEvent,
    SearchHighlight, SearchItem, SearchPalette, rank_search_palette_indices,
    rank_search_palette_indices_with_score,
};
pub use select::Select;
pub use selection::TripleClickSelectionMode;
pub use sequence_diagram::{
    ActivationTheme, ActorKind, ActorRef, AutonumberTheme, FragmentGlyphs, FragmentKind,
    LifelineTheme, MessageGlyphs, MessageStyle, Msg, NotePlacement, SequenceDiagram,
    SequenceDiagramTheme, SequenceDiagramVariant, SequenceItemEvent, SequenceItemPath,
    SequenceMessage, SequenceStep, Step,
};
pub use slider::Slider;
pub use spacer::Spacer;
pub use sparkline::{
    Sparkline, SparklineAggregation, SparklineBarsPreset, SparklineLineGlyphs, SparklineLinePreset,
    SparklineVariant, SparklineZeroPolicy,
};
pub use spinner::{Spinner, SpinnerSpeed, SpinnerStyle};
pub use splitter::{Splitter, SplitterResizeEvent};
pub use state_diagram::{StateDiagram, StateDiagramTheme, StateKind, StateSpec, StateTransition};
pub use status_bar::StatusBar;
pub use table::{
    ColumnWidth, Table, TableCell, TableDisclosureState, TableEvent, TableRow, TableRowRole,
};
pub(crate) use tabs::tab_width_budgets;
pub use tabs::{Tab, Tabs, TabsEvent, TabsOverflow};
#[cfg(feature = "terminal")]
pub use terminal::{
    MouseEncoding, MouseMode, MouseModeState, Terminal, TerminalBuffer, TerminalColorPalette,
    TerminalInputEvent, TerminalInputKind, TerminalPty, TerminalPtyConfig, TerminalPtyError,
    TerminalPtyEvent, TerminalRenderSnapshot, TerminalScreen, TerminalSelection,
    TerminalSelectionEvent, TerminalViewport, focus_sequences, key_event_to_bytes,
    mouse_event_to_bytes, paste_sequences, wrap_bracketed_paste,
};
pub use text::{Overflow, Text};
pub use text_area::{
    IMAGE_SENTINEL_BASE, SENTINEL_BASE, SentinelEvent, SentinelId, TextArea,
    TextAreaClipboardTransform, TextAreaClipboardTransformEvent, TextAreaColorInput,
    TextAreaColorLines, TextAreaColorStrategy, TextAreaCursorMetrics, TextAreaDecoration,
    TextAreaDecorationKind, TextAreaEvent, TextAreaGutter, TextAreaGutterColumn,
    TextAreaGutterSign, TextAreaImageMode, TextAreaLineNumberMode, TextAreaMetrics,
    TextAreaPasteEvent, TextAreaSentinel, TextAreaSentinelClickEvent, TextAreaSentinelClickKind,
    TextAreaSnapshot, TextAreaStateChangeEvent, TextAreaStateChangeReason, TextAreaVimConfig,
    TextAreaVimCurrentLineHighlight, TextAreaVimKeyBinding, TextAreaVimKeymap, TextAreaVimMode,
    TextAreaVirtualText, VirtualTextPlacement, insert_sentinel,
};
pub(crate) use text_area::{
    TEXT_AREA_LAYER_PRIORITY_CURRENT_SEARCH, TEXT_AREA_LAYER_PRIORITY_SEARCH,
    TEXT_AREA_LAYER_PRIORITY_SELECTION, TextAreaColorCache, TextAreaLayerKind, TextAreaRangeLayer,
    TextAreaStyledSegment, TextAreaVimSearchFeedback, TextAreaVisualCache, TextAreaVisualKeyArgs,
    TextAreaVisualLine, VirtualTextLayoutCtx, eol_virtual_texts_for_visual_line,
    hash_peer_source_lines, inline_virtual_insertions_for_line,
    inline_virtual_texts_for_visual_line, layout_line_with_inline_virtual_text,
    make_text_area_visual_key, public_decoration_layers_for_visible_range, resolve_text_area_spans,
    segments_from_plain, segments_from_spans, sentinel_info_for, text_area_total_gutter_width,
    text_area_virtual_text_hash,
};
pub use theme_provider::ThemeProvider;

#[cfg(feature = "syntax-syntect")]
pub use text_area::{
    SyntectDocumentFormatter, SyntectStrategy, apply_syntect_strategy_app_theme, language_from_path,
};
pub use toast::{Toast, ToastCopyAffordance};
pub use tooltip::Tooltip;
pub use tree::{IndentStyle, Tree, TreeEvent, TreeKeymap, TreeNode, TreePath, TreeToggleEvent};
pub use zstack::ZStack;

pub(crate) mod internal;
