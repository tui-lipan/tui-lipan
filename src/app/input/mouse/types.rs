use crate::callback::Callback;
use crate::core::event::{MouseDragEvent, MouseEvent};
use crate::core::node::NodeId;
use crate::style::{Padding, Rect, ScrollbarVariant};
use crate::utils::text::SentinelInfo;
#[cfg(feature = "diff-view")]
use crate::widgets::DiffContextSeparatorEvent;
use crate::widgets::{
    CheckboxEvent, CheckboxState, DocumentClickEvent, DragReorderMode, DraggableTabActionEvent,
    DraggableTabCloseEvent, DraggableTabTransferEvent, FlowchartEdgeEvent, FlowchartNodeEvent,
    FlowchartSubgraphEvent, InputEvent, ListEvent, ProgressEvent, TableRow, TabsEvent,
    TextAreaSentinelClickEvent,
};
use crate::widgets::{TextAreaImageMode, TextAreaSentinel, TextAreaVisualLine};
use std::sync::Arc;

/// Information gathered from hit-testing a mouse click.
pub(crate) struct HitActions {
    pub on_click: Option<Callback<MouseEvent>>,
    pub on_mouse_down: Option<Callback<MouseEvent>>,
    pub on_mouse_up: Option<Callback<MouseEvent>>,
    pub on_drag_start: Option<Callback<MouseDragEvent>>,
    pub on_drag: Option<Callback<MouseDragEvent>>,
    pub on_drag_end: Option<Callback<MouseDragEvent>>,
    pub document_click: Option<DocumentClick>,
    pub input_change: Option<InputChange>,
    pub list_select: Option<ListSelect>,
    pub table_select: Option<TableSelect>,
    pub tabs_change: Option<TabsChange>,
    pub draggable_tab_bar_action: Option<DraggableTabBarAction>,
    pub border_tabs_change: Option<TabsChange>,
    pub checkbox_toggle: Option<CheckboxToggle>,
    pub progress_change: Option<ProgressChange>,
    pub graph_node_click: Option<GraphNodeClick>,
    pub sequence_item_click: Option<SequenceItemClick>,
    pub flowchart_item_click: Option<FlowchartItemClick>,
    #[cfg(feature = "diff-view")]
    pub diff_context_separator_click: Option<DiffContextSeparatorClick>,
    pub slider_change: Option<SliderChange>,
    pub splitter_grab: Option<SplitterGrab>,
    pub drag_source_grab: Option<DragSourceGrab>,
    pub textarea_change: Option<TextAreaChange>,
}

pub(crate) struct DocumentClick {
    pub cb: Callback<DocumentClickEvent>,
    pub source_line: usize,
    pub link: Option<Arc<str>>,
}

#[cfg(feature = "diff-view")]
pub(crate) struct DiffContextSeparatorClick {
    pub cb: Callback<DiffContextSeparatorEvent>,
    pub event: DiffContextSeparatorEvent,
}

pub(crate) struct InputChange {
    pub on_change: Option<Callback<InputEvent>>,
    pub value: Arc<str>,
    pub cursor: usize,
    pub anchor: Option<usize>,
    pub focusable: bool,
    pub prefix: Option<Arc<str>>,
    pub border: bool,
    pub padding: Padding,
    pub rect: Rect,
    pub node_id: NodeId,
    pub read_only: bool,
    pub masked: bool,
}

pub(crate) struct ListSelect {
    pub cb: Callback<ListEvent>,
    pub on_item_click: Option<Callback<ListEvent>>,
    pub on_activate: Option<Callback<ListEvent>>,
    pub activate_on_click: bool,
    pub len: usize,
    pub border: bool,
    pub padding: Padding,
    pub scrollbar: bool,
    pub scrollbar_variant: ScrollbarVariant,
    pub show_scroll_indicators: bool,
    pub rect: Rect,
}

pub(crate) struct TableSelect {
    pub cb: Callback<crate::widgets::TableEvent>,
    pub on_activate: Option<Callback<crate::widgets::TableEvent>>,
    pub rows: Arc<[TableRow]>,
    pub offset: usize,
    pub header_height: u16,
    pub row_gap: u16,
    pub rect: Rect,
    pub border: bool,
    pub padding: Padding,
    pub show_scroll_indicators: bool,
    pub top_indicator: bool,
    pub bottom_indicator: bool,
}

pub(crate) struct TabsChange {
    pub cb: Callback<TabsEvent>,
    pub next: usize,
    pub active: usize,
}

pub(crate) struct DraggableTabBarAction {
    pub node_id: NodeId,
    pub overflow_scroll_step: i8,
    pub tab_index: usize,
    pub action_hit: bool,
    pub close_hit: bool,
    pub active: usize,
    pub bar_id: Option<Arc<str>>,
    pub drag_group: Option<Arc<str>>,
    pub draggable: bool,
    pub reorder_mode: DragReorderMode,
    pub drag_threshold: u16,
    pub on_change: Option<Callback<TabsEvent>>,
    pub on_action: Option<Callback<DraggableTabActionEvent>>,
    pub on_close: Option<Callback<DraggableTabCloseEvent>>,
    pub on_transfer: Option<Callback<DraggableTabTransferEvent>>,
}

pub(crate) struct CheckboxToggle {
    pub cb: Callback<CheckboxEvent>,
    pub state: CheckboxState,
}

pub(crate) struct ProgressChange {
    pub on_change: Option<Callback<ProgressEvent>>,
    pub progress: f64,
    pub node_id: NodeId,
    pub draggable: bool,
}

pub(crate) struct GraphNodeClick {
    pub node_id: NodeId,
    pub cb: Option<Callback<crate::widgets::GraphNodeEvent>>,
    pub event: crate::widgets::GraphNodeEvent,
}

pub(crate) struct SequenceItemClick {
    pub cb: Callback<crate::widgets::SequenceItemEvent>,
    pub event: crate::widgets::SequenceItemEvent,
}

pub(crate) enum FlowchartItemClick {
    Node {
        cb: Callback<FlowchartNodeEvent>,
        event: FlowchartNodeEvent,
    },
    Edge {
        cb: Callback<FlowchartEdgeEvent>,
        event: FlowchartEdgeEvent,
    },
    Subgraph {
        cb: Callback<FlowchartSubgraphEvent>,
        event: FlowchartSubgraphEvent,
    },
}

pub(crate) struct SliderChange {
    pub node_id: NodeId,
}

pub(crate) struct SplitterGrab {
    pub node_id: NodeId,
    pub handle: usize,
}

pub(crate) struct DragSourceGrab {
    pub node_id: NodeId,
}

pub(crate) struct TextAreaChange {
    pub on_change: Option<Callback<crate::widgets::TextAreaEvent>>,
    pub on_editor_state_change: Option<Callback<crate::widgets::TextAreaStateChangeEvent>>,
    pub value: Arc<str>,
    pub cursor: usize,
    pub anchor: Option<usize>,
    pub focusable: bool,
    pub border: bool,
    pub padding: Padding,
    pub line_numbers: bool,
    pub min_line_number_width: u8,
    pub wrap: bool,
    pub scroll_offset: usize,
    pub scrollbar: bool,
    pub scrollbar_variant: ScrollbarVariant,
    pub scrollbar_gap: u16,
    /// True if an integrated scrollbar is rendered over a border (own border or parent Frame border).
    /// In this mode, the scrollbar does not consume content width.
    pub scrollbar_over_border: bool,
    pub h_scrollbar: bool,
    pub h_scrollbar_variant: ScrollbarVariant,
    pub h_scrollbar_over_border: bool,
    pub max_line_width: usize,
    pub h_scroll_offset: usize,
    pub rect: Rect,
    pub node_id: NodeId,
    pub read_only: bool,
    pub vim_motions: bool,
    pub on_vim_mode_change: Option<Callback<crate::widgets::TextAreaVimMode>>,
    /// Sentinel info for inline image placeholder width accounting.
    pub sentinel_info: Option<SentinelInfo>,
    pub tab_stop: usize,
    /// Custom gutter column width (0 = use line_numbers-derived width).
    pub gutter_col_width: u16,
    pub gutter_gap: u16,
    pub logical_lines_count: usize,
    pub visual_lines: Option<Arc<[TextAreaVisualLine]>>,
    pub virtual_texts: Vec<crate::widgets::TextAreaVirtualText>,
    pub multi_click_select: bool,
    pub triple_click_mode: crate::widgets::TripleClickSelectionMode,
    pub on_sentinel_click: Option<Callback<TextAreaSentinelClickEvent>>,
    #[cfg(feature = "diff-view")]
    pub diff_context_separator_click: Option<crate::widgets::DiffContextSeparatorClickConfig>,
    pub images: Vec<crate::clipboard::ImageContent>,
    pub image_mode: TextAreaImageMode,
    pub sentinels: Vec<TextAreaSentinel>,
}
