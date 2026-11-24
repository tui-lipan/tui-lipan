//! Compile-time contract tests for crate-root exports and prelude hygiene.

#[test]
fn crate_root_exports_common_widgets_and_helpers() {
    #[allow(unused_imports)]
    use tui_lipan::{
        Accordion, Badge, Breadcrumb, Button, Checkbox, ClassDiagram, CodeBlockHeader,
        CodeBlockPadding, Color, ColorGradient, DEFAULT_COLOR_ANIMATION_FRAME_RATE,
        DEFAULT_FRAME_RATE, DocumentView, Easing, Edge, ErDiagram, FileTree, FlowDirection,
        Flowchart, FlowchartEdge, FlowchartNodeId, Frame, GanttDiagram, Graph, GraphNode, HStack,
        Heatmap, HexArea, Input, List, ListItem, Modal, NodeId, NodeShape, Popover, ProgressBar,
        Radio, RadioEvent, ScrollView, SearchPalette, Select, SelectEvent, SelectionEnd,
        SequenceDiagram, SequenceMessage, SequenceStep, Slider, Spinner, StateDiagram, StatusBar,
        Tab, Table, TableCell, TableRow, Tabs, Text, TextArea, TextEditor, TextInput, Tooltip,
        Tree, TreeNode, TripleClickSelectionMode, VStack, word_forward_start,
    };

    let _ = (
        DEFAULT_FRAME_RATE,
        DEFAULT_COLOR_ANIMATION_FRAME_RATE,
        Easing::Linear,
        Edge::Top,
        std::any::type_name::<SelectionEnd>(),
        word_forward_start("hello world", 0),
        ColorGradient::new(Color::Blue, Color::Cyan),
    );

    let _ = TextInput::new("");
    let _ = TextEditor::new("");
    let _ = Flowchart::new(FlowDirection::TopDown)
        .node(FlowchartNodeId::new("a"), "A", NodeShape::Rect)
        .node("b", "B", NodeShape::Rect)
        .edge(FlowchartEdge::solid("a", "b"));
    let _ = SequenceDiagram::new()
        .participant("A")
        .participant("B")
        .message(SequenceMessage::sync("A", "B", "ping"))
        .step(SequenceStep::self_msg("B", "work"));
    let _ = Button::new("OK");
    let _ = VStack::new().child(HStack::new().child(Text::new("hi")));
    let _ = List::new().items([ListItem::new("one")]);
    let _ = Input::new("");
    let _ = Modal::new();
    let _ = Tabs::new().tab("One");
    let _ = Heatmap::new([[1.0_f64]]);
    let _ = HexArea::new(vec![0u8]);
    let _ = TripleClickSelectionMode::default();
    let _ = CodeBlockHeader::default();
    let _ = CodeBlockPadding::default();
}

#[cfg(feature = "big-text")]
#[test]
fn crate_root_exports_big_text_feature_items() {
    #[allow(unused_imports)]
    use tui_lipan::{BigFont, BigText, GlyphLayout, Shadow};
    let _ = BigText::new();
}

#[cfg(feature = "diff-view")]
#[test]
fn crate_root_exports_diff_view_feature_items() {
    #[allow(unused_imports)]
    use tui_lipan::{DiffData, DiffView, DiffViewMode};
    let _ = std::any::type_name::<DiffView>();
    let _ = std::any::type_name::<DiffViewMode>();
}

#[cfg(feature = "image")]
#[test]
fn crate_root_exports_image_feature_items() {
    #[allow(unused_imports)]
    use tui_lipan::{Image, ImageFit, ImageProtocol, ImageSource};
    let _ = ImageFit::default();
}

#[cfg(feature = "markdown")]
#[test]
fn crate_root_exports_markdown_feature_items() {
    use tui_lipan::{MarkdownFormatter, MarkdownSpacing, SourceSpacing};
    let _ = std::any::type_name::<MarkdownFormatter>();
    let _ = MarkdownSpacing {
        heading_after: SourceSpacing::Never,
        ..MarkdownSpacing::BALANCED
    };
}

#[cfg(feature = "qr-code")]
#[test]
fn crate_root_exports_qr_code_feature_items() {
    #[allow(unused_imports)]
    use tui_lipan::{QrCode, QrEcc, QrRender};
    let _ = QrCode::new("https://example.com");
}

#[cfg(feature = "syntax-syntect")]
#[test]
fn crate_root_exports_syntect_feature_items() {
    #[allow(unused_imports)]
    use tui_lipan::{SyntectDocumentFormatter, SyntectStrategy, language_from_path};
    let _ = language_from_path("main.rs");
}

#[cfg(feature = "terminal")]
#[test]
fn crate_root_exports_terminal_feature_items() {
    #[allow(unused_imports)]
    use tui_lipan::{ManagedTerminal, Terminal, TerminalPty, TerminalScreen, key_event_to_bytes};
    let _ = std::any::type_name::<Terminal>();
    let _ = std::any::type_name::<ManagedTerminal>();
}

#[cfg(feature = "terminal-images")]
#[test]
fn crate_root_exports_terminal_images_feature_items() {
    #[allow(unused_imports)]
    use tui_lipan::{GraphicsMediaPolicy, TerminalImage, host_cell_size, host_reads_shared_frames};
    let _ = std::any::type_name::<TerminalImage>();
}

mod prelude_hygiene {
    use std::any::TypeId;
    use tui_lipan::prelude::*;
    use tui_lipan::{FlowchartNodeId, SequenceStep};

    // Common user-authored names must not collide with prelude re-exports.
    #[allow(dead_code)]
    enum Msg {
        Ping,
    }

    #[allow(dead_code)]
    struct Step;

    #[derive(Default)]
    struct State {
        focused: Option<NodeId>,
    }

    struct FocusReader;

    impl Component for FocusReader {
        type Message = Msg;
        type Properties = ();
        type State = State;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            State::default()
        }

        fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
            match msg {
                Msg::Ping => {
                    let id: Option<NodeId> = ctx.focused_node_id();
                    ctx.state.focused = id;
                    Update::none()
                }
            }
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            let _ = Edge::Top;
            let _ = FlowchartEdge::solid(FlowchartNodeId::new("a"), "b");
            let _ = SequenceMessage::sync("A", "B", "hi");
            let _ = SequenceStep::self_msg("A", "work");
            let _ = Step;
            Text::new("ok").into()
        }
    }

    #[test]
    fn prelude_node_id_and_edge_match_crate_root() {
        assert_eq!(TypeId::of::<NodeId>(), TypeId::of::<tui_lipan::NodeId>());
        assert_ne!(
            TypeId::of::<NodeId>(),
            TypeId::of::<FlowchartNodeId>(),
            "core NodeId and FlowchartNodeId must be distinct types"
        );
        assert_eq!(TypeId::of::<Edge>(), TypeId::of::<tui_lipan::Edge>());
        assert_ne!(
            TypeId::of::<Edge>(),
            TypeId::of::<FlowchartEdge>(),
            "style Edge and FlowchartEdge must be distinct types"
        );

        let mut backend = tui_lipan::TestBackend::new(FocusReader);
        backend.render();
    }
}
